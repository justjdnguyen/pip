//! Brief, independent resistance on each axis during a native window drag.
use std::time::Duration;

pub const HOLD: Duration = Duration::from_millis(220);

#[derive(Default)]
pub struct Axis {
    held: Option<Contact>,
    bypass: Option<i32>,
}

struct Contact {
    target: i32,
    direction: i32,
    since: Duration,
}

impl Axis {
    /// Positions and targets are unsnapped screen coordinates. After breaking
    /// through, ignore that edge until the pointer has left its capture range.
    pub fn apply(
        &mut self,
        previous: i32,
        position: i32,
        targets: &[i32],
        radius: i32,
        now: Duration,
    ) -> i32 {
        if let Some(contact) = &self.held {
            let beyond = (position - contact.target) * contact.direction;
            if !targets.contains(&contact.target) || beyond < -radius {
                self.held = None;
            } else if beyond >= radius && now.saturating_sub(contact.since) >= HOLD {
                self.bypass = Some(contact.target);
                self.held = None;
                return position;
            } else {
                return contact.target;
            }
        }
        if self
            .bypass
            .is_some_and(|t| (position - t).abs() > radius * 2)
        {
            self.bypass = None;
        }
        let direction = (position - previous).signum();
        if direction == 0 {
            return position;
        }
        // Include crossed edges, so a quick mouse movement cannot skip a seam.
        let target = targets
            .iter()
            .copied()
            .filter(|&t| {
                Some(t) != self.bypass
                    && (t - previous) * direction > 0
                    && (t - position) * direction <= radius
            })
            .min_by_key(|t| (t - previous).abs());
        if let Some(target) = target {
            self.held = Some(Contact {
                target,
                direction,
                since: now,
            });
            target
        } else {
            position
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn catches_fast_crossing_and_releases_after_continued_motion() {
        let mut axis = Axis::default();
        assert_eq!(axis.apply(50, 140, &[100], 10, ms(0)), 100);
        assert_eq!(axis.apply(140, 150, &[100], 10, ms(219)), 100);
        assert_eq!(axis.apply(150, 160, &[100], 10, ms(220)), 160);
        assert_eq!(axis.apply(160, 165, &[100], 10, ms(230)), 165);
    }

    #[test]
    fn stays_aligned_if_pointer_stops_near_edge() {
        let mut axis = Axis::default();
        assert_eq!(axis.apply(50, 95, &[100], 10, ms(0)), 100);
        assert_eq!(axis.apply(95, 103, &[100], 10, ms(500)), 100);
        assert_eq!(axis.apply(103, 112, &[100], 10, ms(510)), 112);
        assert_eq!(axis.apply(112, 108, &[100], 10, ms(520)), 108);
    }

    #[test]
    fn can_pull_back_or_lose_a_target_without_waiting() {
        let mut axis = Axis::default();
        assert_eq!(axis.apply(50, 98, &[100], 10, ms(0)), 100);
        assert_eq!(axis.apply(98, 80, &[100], 10, ms(10)), 80);
        assert_eq!(axis.apply(80, 98, &[100], 10, ms(20)), 100);
        assert_eq!(axis.apply(98, 102, &[], 10, ms(30)), 102);
    }

    #[test]
    fn negative_coordinates_choose_first_edge_in_direction_of_travel() {
        let mut axis = Axis::default();
        assert_eq!(axis.apply(0, -200, &[-150, -100], 10, ms(0)), -100);
    }

    #[test]
    fn dragging_away_from_an_aligned_edge_is_free() {
        let mut axis = Axis::default();
        assert_eq!(axis.apply(100, 105, &[100], 10, ms(0)), 105);
    }
}
