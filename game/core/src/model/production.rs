use super::*;
impl Simulation {
    pub(super) fn sources(&mut self) {
        for i in 0..self.config.sources.len() {
            let s = self.config.sources[i].clone();
            if self.tick % ticks(s.every) != 0 {
                continue;
            }
            let units = s.units.min(self.free(&s.depot)).min(
                s.max_units
                    .map_or(u32::MAX, |m| m.saturating_sub(self.source_totals[i])),
            );
            if units > 0 {
                self.depots.get_mut(&s.depot).unwrap().add(&s.item, units);
                self.accounts.get_mut(&s.item).unwrap().generated += units as u64;
                self.source_totals[i] += units;
            }
        }
    }
    pub(super) fn production(&mut self) {
        for p in self.config.producers.clone() {
            let mut state = self.producers[&p.id].clone();
            if state.remaining > 0 {
                state.remaining -= 1;
                if state.remaining == 0 {
                    for s in &p.products {
                        self.depots
                            .get_mut(&p.output)
                            .unwrap()
                            .add(&s.item, s.units);
                        self.accounts.get_mut(&s.item).unwrap().produced += s.units as u64;
                    }
                    state.cycles += 1;
                    self.metrics.cycles += 1;
                    self.events.push(Event::Produced {
                        producer: p.id.clone(),
                        products: p.products.clone(),
                    });
                }
            } else if state.enabled && p.max_cycles.is_none_or(|limit| state.cycles < limit) {
                let required: u32 = p.products.iter().map(|s| s.units).sum();
                let freed = if p.input == p.output {
                    p.ingredients.iter().map(|s| s.units).sum()
                } else {
                    0
                };
                if self.free(&p.output) + freed >= required
                    && p.ingredients
                        .iter()
                        .all(|s| self.available(&p.input, &s.item) >= s.units)
                {
                    for s in &p.ingredients {
                        self.depots
                            .get_mut(&p.input)
                            .unwrap()
                            .take(&s.item, s.units);
                        self.accounts.get_mut(&s.item).unwrap().consumed += s.units as u64;
                    }
                    state.remaining = ticks(p.seconds);
                }
            }
            self.producers.insert(p.id, state);
        }
    }
}
