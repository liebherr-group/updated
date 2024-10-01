// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::time::Duration;

use tokio::time::{Instant, sleep_until};
use tracing::info;

use crate::{LOG, update_trigger::UpdateTrigger};

/// Rate implements an update trigger that fires with a defined rate, e.g.
/// every hour.
pub struct Rate<T> {
    /// the duration between two updates
    update_interval: Duration,
    /// the returned value if the update triggers
    trigger_value: T,
    /// the time, the next update is scheduled at
    next_trigger: Instant,
}

impl<T> Rate<T> {
    /// creates a new Rate trigger
    pub fn new(sleep_duration: Duration, trigger_value: T) -> Self {
        Self {
            update_interval: sleep_duration,
            trigger_value,
            next_trigger: Instant::now(),
        }
    }

    /// changes the interval between two updates.
    /// The time at which the next update triggers is recalculated.
    pub fn set_interval(&mut self, interval: Duration) {
        // reschedule the next trigger if the interval changes
        self.next_trigger -= self.update_interval;
        self.next_trigger += interval;
        self.update_interval = interval;
    }
}

impl<T: Clone> UpdateTrigger<T> for Rate<T> {
    async fn update_triggered(&mut self) -> Result<T, crate::update_trigger::UpdateTriggerError> {
        sleep_until(self.next_trigger).await;
        self.next_trigger = Instant::now() + self.update_interval;
        info!(target: LOG, "Slept for {} seconds. Time to check for updates.", self.update_interval.as_secs());
        Ok(self.trigger_value.clone())
    }
}

/// Update trigger that starts an update immediately.
pub struct Immediately<T: Clone> {
    value: T,
}

impl<T: Clone> Immediately<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T: Clone> UpdateTrigger<T> for Immediately<T> {
    async fn update_triggered(&mut self) -> Result<T, crate::update_trigger::UpdateTriggerError> {
        Ok(self.value.clone())
    }
}

#[test]
fn test_change_rate() {
    // Test to ensure the rate trigger interval can be changed
    let mut trigger = Rate::new(Duration::from_secs(5), ());
    let next_trigger = trigger.next_trigger;
    trigger.set_interval(Duration::from_secs(30)); // increase the interval
    assert_eq!(trigger.next_trigger, next_trigger + Duration::from_secs(25));
    trigger.set_interval(Duration::from_secs(10)); // decrease the interval
    assert_eq!(trigger.next_trigger, next_trigger + Duration::from_secs(5));
}
