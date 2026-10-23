use super::*;
impl Simulation {
    pub(super) fn pending_cargo(&self, agent: &str) -> bool {
        self.transfers
            .values()
            .any(|t| matches!(&t.to,Endpoint::Cargo(id) if id==agent))
    }
    pub(super) fn pending_drop(&self, agent: &str) -> bool {
        self.transfers
            .values()
            .any(|t| matches!(&t.from,Endpoint::Cargo(id) if id==agent))
    }
    pub(super) fn radius(def: &Agent, state: &AgentState) -> f32 {
        if state.equipment.is_some() {
            def.radius.max(0.35)
        } else {
            def.radius
        }
    }
    pub(super) fn plan(&self, from: Point, to: Point, radius: f32) -> Vec<Point> {
        navigation::path(from, to, &self.obstacles, radius)
    }
    pub(super) fn assign(&mut self, def: &Agent, a: &mut AgentState) {
        if self.tick < a.retries {
            return;
        }
        let radius = Self::radius(def, a);
        if a.equipment.is_none() {
            if let Some(e) = &def.equipment {
                if self.available(&e.depot, &e.item) > 0 {
                    let route = self.plan(a.position, self.depot_position(&e.depot), radius);
                    if !route.is_empty() {
                        a.phase = Phase::ToEquip;
                        a.route = route;
                        return;
                    }
                }
            }
        }
        let mut links: Vec<_> = self
            .config
            .links
            .iter()
            .enumerate()
            .filter(|(_, l)| def.links.contains(&l.id) && self.links[&l.id])
            .map(|(i, l)| (i, l.clone()))
            .collect();
        links.sort_by_key(|(i, l)| (std::cmp::Reverse(l.priority), *i));
        for (index, l) in links {
            let demand = l.target.saturating_sub(
                self.depots[&l.to].units(&l.item) + self.incoming(&l.to, Some(&l.item)),
            );
            let units = self
                .available(&l.from, &l.item)
                .min(self.free(&l.to))
                .min(demand)
                .min(a.cargo.capacity);
            if units == 0 {
                continue;
            }
            let from = self.depot_position(&l.from);
            let to = self.depot_position(&l.to);
            let route = self.plan(a.position, from, radius);
            if route.is_empty() || self.plan(from, to, radius).is_empty() {
                continue;
            }
            a.assignment = Some(Assignment {
                link: index,
                units,
                loaded: 0,
                unloaded: 0,
            });
            a.phase = Phase::ToPickup;
            a.route = route;
            return;
        }
        a.retries = self.tick + 15;
    }
    pub(super) fn launch(
        &mut self,
        item: &str,
        from: Endpoint,
        to: Endpoint,
        start: Point,
        end: Point,
    ) {
        let id = self.id();
        self.transfers.insert(
            id,
            Transfer {
                id,
                item: item.into(),
                from,
                to,
                start,
                end,
                born: self.tick,
                arrival: self.tick + 21,
            },
        );
        self.events.push(Event::TransferStarted(id));
    }
    pub(super) fn workers(&mut self) {
        let mut docks: BTreeMap<String, String> = BTreeMap::new();
        for def in &self.config.agents {
            let a = &self.agents[&def.id];
            let dock = match a.phase {
                Phase::Loading => a
                    .assignment
                    .as_ref()
                    .map(|j| self.config.links[j.link].from.clone()),
                Phase::Unloading => a
                    .assignment
                    .as_ref()
                    .map(|j| self.config.links[j.link].to.clone()),
                Phase::Attaching => def.equipment.as_ref().map(|e| e.depot.clone()),
                _ => None,
            };
            if let Some(d) = dock {
                docks.insert(d, a.id.clone());
            }
        }
        for def in self.config.agents.clone() {
            if def.controlled {
                continue;
            }
            let mut a = self.agents.remove(&def.id).unwrap();
            match a.phase {
                Phase::Idle => self.assign(&def, &mut a),
                Phase::ToPickup | Phase::ToDrop | Phase::ToEquip => {
                    let dock = if a.phase == Phase::ToEquip {
                        def.equipment.as_ref().unwrap().depot.clone()
                    } else {
                        let j = a.assignment.as_ref().unwrap();
                        let l = &self.config.links[j.link];
                        if a.phase == Phase::ToPickup {
                            l.from.clone()
                        } else {
                            l.to.clone()
                        }
                    };
                    let goal = self.depot_position(&dock);
                    let busy = docks.get(&dock).is_some_and(|id| id != &a.id);
                    if !(busy && navigation::distance(a.position, goal) < 0.75) {
                        let speed = def.speed
                            * if a.equipment.is_some() {
                                def.equipment.as_ref().map_or(1.0, |e| e.speed_factor)
                            } else {
                                1.0
                            };
                        let before = a.position;
                        move_route(&mut a.position, &mut a.route, speed / HZ as f32);
                        let delta = [a.position[0] - before[0], a.position[1] - before[1]];
                        let distance = navigation::distance(a.position, before);
                        a.travelled += distance;
                        if distance > 0.00001 {
                            a.heading = delta[0].atan2(delta[1]);
                        }
                        if let Some(trailer) = &mut a.trailer {
                            let hitch = def.equipment.as_ref().unwrap().offset[2].abs().max(0.6);
                            let d = navigation::distance(trailer.position, a.position);
                            if d > hitch {
                                let previous = trailer.position;
                                let direction = [
                                    (a.position[0] - trailer.position[0]) / d,
                                    (a.position[1] - trailer.position[1]) / d,
                                ];
                                trailer.position = [
                                    a.position[0] - direction[0] * hitch,
                                    a.position[1] - direction[1] * hitch,
                                ];
                                trailer.heading = direction[0].atan2(direction[1]);
                                trailer.travelled +=
                                    navigation::distance(previous, trailer.position);
                            }
                        }
                        if a.route.is_empty() && !busy {
                            docks.insert(dock.clone(), a.id.clone());
                            a.transfer_at = self.tick;
                            if a.phase == Phase::ToPickup {
                                a.phase = Phase::Loading;
                            } else if a.phase == Phase::ToDrop {
                                a.phase = Phase::Unloading;
                            } else {
                                let e = def.equipment.as_ref().unwrap();
                                self.depots.get_mut(&e.depot).unwrap().take(&e.item, 1);
                                a.equipment = Some(e.item.clone());
                                a.trailer = Some(Trailer {
                                    position: [
                                        a.position[0] - a.heading.sin() * e.offset[2].abs(),
                                        a.position[1] - a.heading.cos() * e.offset[2].abs(),
                                    ],
                                    heading: a.heading,
                                    travelled: 0.0,
                                });
                                a.phase = Phase::Attaching;
                                a.transfer_at = self.tick + 60;
                            }
                        }
                    }
                }
                Phase::Loading => {
                    let j = a.assignment.as_mut().unwrap();
                    let l = self.config.links[j.link].clone();
                    if j.loaded < j.units && self.tick >= a.transfer_at {
                        self.depots.get_mut(&l.from).unwrap().take(&l.item, 1);
                        j.loaded += 1;
                        self.metrics.picked += 1;
                        self.launch(
                            &l.item,
                            Endpoint::Depot(l.from.clone()),
                            Endpoint::Cargo(a.id.clone()),
                            self.depot_position(&l.from),
                            a.position,
                        );
                        a.transfer_at = self.tick + ticks(def.transfer_seconds);
                    }
                    if j.loaded == j.units && !self.pending_cargo(&a.id) {
                        a.route = self.plan(
                            a.position,
                            self.depot_position(&l.to),
                            Self::radius(&def, &a),
                        );
                        a.phase = Phase::ToDrop;
                    }
                }
                Phase::Unloading => {
                    let j = a.assignment.as_mut().unwrap();
                    let l = self.config.links[j.link].clone();
                    if j.unloaded < j.units && self.tick >= a.transfer_at {
                        a.cargo.take(&l.item, 1);
                        j.unloaded += 1;
                        self.launch(
                            &l.item,
                            Endpoint::Cargo(a.id.clone()),
                            Endpoint::Depot(l.to.clone()),
                            a.position,
                            self.depot_position(&l.to),
                        );
                        a.transfer_at = self.tick + ticks(def.transfer_seconds);
                    }
                    if j.unloaded == j.units && !self.pending_drop(&a.id) {
                        a.assignment = None;
                        a.phase = Phase::Idle;
                    }
                }
                Phase::Attaching => {
                    if self.tick >= a.transfer_at {
                        a.cargo.capacity = def.equipment.as_ref().unwrap().capacity;
                        a.phase = Phase::Idle;
                        self.metrics.equipped += 1;
                        self.events.push(Event::Equipped {
                            agent: a.id.clone(),
                            item: a.equipment.clone().unwrap(),
                        });
                    }
                }
            }
            self.agents.insert(a.id.clone(), a);
        }
    }
}
