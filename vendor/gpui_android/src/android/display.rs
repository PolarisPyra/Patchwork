//! The phone or tablet screen, the only display an activity has.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use gpui::{Bounds, DisplayId, Pixels, PlatformDisplay, Point, Size};

/// The screen the activity's window fills.
#[derive(Debug, Clone)]
pub(crate) struct AndroidDisplay {
    pub(crate) size: Size<Pixels>,
}

impl PlatformDisplay for AndroidDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::new(0)
    }

    fn uuid(&self) -> anyhow::Result<uuid::Uuid> {
        Ok(uuid::Uuid::nil())
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(Point::default(), self.size)
    }

    fn refresh_interval(&self) -> Option<Duration> {
        // 60 Hz. NativeActivity has no API for the panel's refresh rate.
        Some(Duration::from_micros(16_667))
    }
}
