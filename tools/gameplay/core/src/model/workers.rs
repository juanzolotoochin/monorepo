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
    pub(super) fn plan(&mut self, from: Point, to: Point, radius: f32) -> Vec<Point> {
        self.metrics.navigation_requests += 1;
        let path = navigation::path_with_bounds(
            from,
            to,
            &self.obstacles,
            radius,
            self.config.extent,
            |_, _| true,
        );
        self.metrics.navigation_waypoints += path.len() as u64;
        if path.is_empty() {
            self.metrics.navigation_failures += 1;
        }
        path
    }
    fn plan_agent(&mut self, def: &Agent, a: &AgentState, to: Point) -> Vec<Point> {
        if let (Some(t), Some(e)) = (&a.trailer, &def.equipment) {
            self.metrics.navigation_requests += 1;
            let route = navigation::articulated_path(
                a.position,
                to,
                a.heading,
                t.position,
                e.offset[2].abs().max(0.6),
                &self.obstacles,
                Self::radius(def, a),
                e.radius,
                self.config.extent,
            );
            self.metrics.navigation_waypoints += route.len() as u64;
            if route.is_empty() {
                self.metrics.navigation_failures += 1;
            }
            route
        } else {
            self.plan(a.position, to, Self::radius(def, a))
        }
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
            let route = self.plan_agent(def, a, from);
            if route.is_empty() || self.plan(from, to, radius).is_empty() {
                continue;
            }
            self.metrics.assignments += 1;
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
        self.metrics.assignment_retries += 1;
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
    /// Bounded local steering: retain the global route, steer right around occupied
    /// body/trailer footprints, and stop if no collision-free step exists.
    fn move_worker(&mut self, def: &Agent, a: &mut AgentState, distance: f32) {
        if a.route.is_empty() {
            return;
        }
        let radius = Self::radius(def, a);
        let mut nearby = Vec::new();
        for other in &self.config.agents {
            let Some(b) = self.agents.get(&other.id) else {
                continue;
            };
            if navigation::distance(a.position, b.position) < 3. {
                nearby.push(navigation::Disc {
                    center: b.position,
                    radius: Self::radius(other, b),
                });
            }
            if let Some(t) = &b.trailer {
                if navigation::distance(a.position, t.position) < 3. {
                    nearby.push(navigation::Disc {
                        center: t.position,
                        radius: other.equipment.as_ref().unwrap().radius,
                    });
                }
            }
        }
        if let (Some(t), Some(e)) = (&a.trailer, &def.equipment) {
            nearby.push(navigation::Disc {
                center: t.position,
                radius: e.radius,
            });
        }
        let mut route = a.route.clone();
        let mut direct = a.position;
        move_route(&mut direct, &mut route, distance);
        let dx = direct[0] - a.position[0];
        let dz = direct[1] - a.position[1];
        for angle in [0.0_f32, 0.55, 1.05, 1.55, -0.55, -1.05, -1.55] {
            let (sin, cos) = angle.sin_cos();
            let next = [
                a.position[0] + dx * cos + dz * sin,
                a.position[1] + dz * cos - dx * sin,
            ];
            self.metrics.avoidance_checks += 1;
            let clear = navigation::clear_with_bounds(
                a.position,
                next,
                &self.obstacles,
                radius,
                self.config.extent,
            ) && nearby.iter().all(|o| {
                let before = navigation::distance(a.position, o.center);
                let after = navigation::distance(next, o.center);
                // A player may walk into an NPC: allow that NPC to escape overlap.
                after > radius + o.radius + 0.08 || after > before + 0.00001
            });
            if clear {
                a.position = next;
                if angle == 0. {
                    a.route = route;
                } else {
                    self.metrics.avoidance_steers += 1;
                }
                return;
            }
        }
        self.metrics.traffic_wait_ticks += 1;
    }
    pub(super) fn workers(&mut self) {
        let mut docks: BTreeMap<String, String> = BTreeMap::new();
        for def in &self.config.agents {
            let a = &self.agents[&def.id];
            if let Some(id) = &a.departure_dock {
                let dock = self.depot_position(id);
                let clearance = if a.trailer.is_some() { 2.2 } else { 0.9 };
                if navigation::distance(a.position, dock) < clearance
                    || a.trailer
                        .as_ref()
                        .is_some_and(|t| navigation::distance(t.position, dock) < clearance)
                {
                    docks.insert(id.clone(), a.id.clone());
                }
            }
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
        // Reserve the approach before bodies converge on the exact dock point.
        // Active transfers/departures retain priority; nearest waiting arrival wins.
        let mut approaches = Vec::new();
        for def in &self.config.agents {
            let a = &self.agents[&def.id];
            let dock = match a.phase {
                Phase::ToEquip => def.equipment.as_ref().map(|e| e.depot.clone()),
                Phase::ToPickup | Phase::ToDrop => a.assignment.as_ref().map(|j| {
                    let l = &self.config.links[j.link];
                    if a.phase == Phase::ToPickup {
                        l.from.clone()
                    } else {
                        l.to.clone()
                    }
                }),
                _ => None,
            };
            if let Some(d) = dock {
                approaches.push((
                    navigation::distance(a.position, self.depot_position(&d)),
                    a.id.clone(),
                    d,
                ));
            }
        }
        approaches.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (_, id, dock) in approaches {
            docks.entry(dock).or_insert(id);
        }
        for def in self.config.agents.clone() {
            if def.controlled {
                continue;
            }
            let mut a = self.agents.remove(&def.id).unwrap();
            if let Some(id) = &a.departure_dock {
                let dock = self.depot_position(id);
                let clearance = if a.trailer.is_some() { 2.2 } else { 0.9 };
                if navigation::distance(a.position, dock) >= clearance
                    && a.trailer
                        .as_ref()
                        .is_none_or(|t| navigation::distance(t.position, dock) >= clearance)
                {
                    a.departure_dock = None;
                }
            }
            match a.phase {
                Phase::Idle | Phase::Returning => {
                    self.assign(&def, &mut a);
                    if a.phase == Phase::Idle
                        && navigation::distance(a.position, def.position) > 0.1
                    {
                        a.route = self.plan_agent(&def, &a, def.position);
                        a.phase = Phase::Returning;
                    }
                    if a.phase == Phase::Returning {
                        let before = a.position;
                        let speed = def.speed
                            * if a.equipment.is_some() {
                                def.equipment.as_ref().map_or(1., |e| e.speed_factor)
                            } else {
                                1.
                            };
                        self.move_worker(&def, &mut a, speed / HZ as f32);
                        let d = navigation::distance(before, a.position);
                        a.travelled += d;
                        self.metrics.empty_travel_meters += d as f64;
                        if d > 0.00001 {
                            a.heading =
                                (a.position[0] - before[0]).atan2(a.position[1] - before[1]);
                        }
                        if a.route.is_empty() {
                            a.phase = Phase::Idle;
                        }
                    }
                }
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
                    let queue_distance = docks
                        .get(&dock)
                        .and_then(|id| self.agents.get(id))
                        .map_or(1.5, |owner| if owner.trailer.is_some() { 3.0 } else { 1.5 });
                    if a.route.is_empty()
                        && navigation::distance(a.position, goal) > 0.1
                        && self.tick >= a.retries
                    {
                        a.route = self.plan_agent(&def, &a, goal);
                        a.retries = self.tick + 60;
                    }
                    if busy && navigation::distance(a.position, goal) < queue_distance {
                        self.metrics.dock_wait_ticks += 1;
                        // A larger carrier may take ownership while a pedestrian is
                        // already queued close by. Back out of its turning envelope.
                        let d = navigation::distance(a.position, goal).max(0.01);
                        if d < queue_distance - 0.1 {
                            let route = std::mem::take(&mut a.route);
                            a.route = vec![std::array::from_fn(|i| {
                                (goal[i] + (a.position[i] - goal[i]) * (queue_distance + 0.1) / d)
                                    .clamp(-self.config.extent + 0.1, self.config.extent - 0.1)
                            })];
                            let before = a.position;
                            self.move_worker(&def, &mut a, def.speed / HZ as f32);
                            let moved = navigation::distance(before, a.position);
                            a.travelled += moved;
                            if a.cargo.total() > 0 {
                                self.metrics.loaded_travel_meters += moved as f64;
                            } else {
                                self.metrics.empty_travel_meters += moved as f64;
                            }
                            if moved > 0.00001 {
                                a.heading =
                                    (a.position[0] - before[0]).atan2(a.position[1] - before[1]);
                            }
                            a.route = route;
                        }
                    }
                    if !(busy && navigation::distance(a.position, goal) < queue_distance) {
                        let speed = def.speed
                            * if a.equipment.is_some() {
                                def.equipment.as_ref().map_or(1.0, |e| e.speed_factor)
                            } else {
                                1.0
                            };
                        let before = a.position;
                        self.move_worker(&def, &mut a, speed / HZ as f32);
                        let delta = [a.position[0] - before[0], a.position[1] - before[1]];
                        let distance = navigation::distance(a.position, before);
                        a.travelled += distance;
                        if a.cargo.total() > 0 {
                            self.metrics.loaded_travel_meters += distance as f64;
                        } else {
                            self.metrics.empty_travel_meters += distance as f64;
                        }
                        if distance > 0.00001 {
                            a.heading = delta[0].atan2(delta[1]);
                        }
                        if a.route.is_empty()
                            && !busy
                            && navigation::distance(a.position, goal) <= 0.1
                        {
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
                        a.departure_dock = Some(l.from.clone());
                        a.route = self.plan_agent(&def, &a, self.depot_position(&l.to));
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
                        a.departure_dock = Some(l.to.clone());
                        a.assignment = None;
                        a.phase = Phase::Idle;
                    }
                }
                Phase::Attaching => {
                    if self.tick >= a.transfer_at {
                        a.departure_dock = Some(def.equipment.as_ref().unwrap().depot.clone());
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
                    trailer.travelled += navigation::distance(previous, trailer.position);
                }
            }

            self.agents.insert(a.id.clone(), a);
        }
    }
}
