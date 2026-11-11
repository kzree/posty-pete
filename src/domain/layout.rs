use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};

pub const SIDEBAR_DEFAULT_WIDTH: f32 = 280.;
pub const SIDEBAR_MIN_WIDTH: f32 = 200.;
pub const SIDEBAR_MAX_WIDTH: f32 = 400.;
/// The narrowest the main pane may be left by a restored sidebar.
pub const MAIN_MIN_WIDTH: f32 = 320.;
/// The smallest either side of the request/response split may be restored to.
/// Kept above gpui-base's 100px panel placeholder size, which a pane must not
/// be set to.
pub const SPLIT_PANE_MIN: f32 = 120.;

/// Pane sizes kept across restarts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Layout {
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    /// The request editor's share of the request/response split, from 0 to 1.
    /// `None` until the split is moved, which keeps the even default.
    pub request_split: Option<f32>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            sidebar_collapsed: false,
            request_split: None,
        }
    }
}

impl Layout {
    /// The sidebar width to restore in a window `window_width` wide.
    pub fn sidebar_width_for(&self, window_width: f32) -> f32 {
        let max = (window_width - MAIN_MIN_WIDTH).min(SIDEBAR_MAX_WIDTH);
        let width = if self.sidebar_width.is_finite() {
            self.sidebar_width
        } else {
            SIDEBAR_DEFAULT_WIDTH
        };
        width.min(max).max(SIDEBAR_MIN_WIDTH)
    }

    /// The request editor height to restore in a split `container` tall, if
    /// the split was ever moved and has room for both panes.
    pub fn request_height_for(&self, container: f32) -> Option<f32> {
        let split = self.request_split.filter(|split| split.is_finite())?;
        if !container.is_finite() || container < 2. * SPLIT_PANE_MIN {
            return None;
        }
        Some((split.clamp(0., 1.) * container).clamp(SPLIT_PANE_MIN, container - SPLIT_PANE_MIN))
    }

    /// The split to keep for panes `request` and `response` tall.
    pub fn split_of(request: f32, response: f32) -> Option<f32> {
        let total = request + response;
        (request.is_finite() && total.is_finite() && total > 0.)
            .then(|| (request / total).clamp(0., 1.))
    }
}

/// Fields that are missing or malformed keep their defaults, rather than
/// failing the whole file.
impl<'de> Deserialize<'de> for Layout {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let serde_json::Value::Object(mut fields) = serde_json::Value::deserialize(deserializer)?
        else {
            return Ok(Self::default());
        };
        let default = Self::default();
        Ok(Self {
            sidebar_width: field(&mut fields, "sidebar_width").unwrap_or(default.sidebar_width),
            sidebar_collapsed: field(&mut fields, "sidebar_collapsed")
                .unwrap_or(default.sidebar_collapsed),
            request_split: field(&mut fields, "request_split").unwrap_or(default.request_split),
        })
    }
}

fn field<T: DeserializeOwned>(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Option<T> {
    serde_json::from_value(fields.remove(name)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidebar(width: f32) -> Layout {
        Layout {
            sidebar_width: width,
            ..Layout::default()
        }
    }

    fn split(split: f32) -> Layout {
        Layout {
            request_split: Some(split),
            ..Layout::default()
        }
    }

    #[test]
    fn sidebar_width_is_kept_within_its_range() {
        assert_eq!(sidebar(250.).sidebar_width_for(1400.), 250.);
        assert_eq!(sidebar(50.).sidebar_width_for(1400.), SIDEBAR_MIN_WIDTH);
        assert_eq!(sidebar(900.).sidebar_width_for(1400.), SIDEBAR_MAX_WIDTH);
        assert_eq!(
            sidebar(f32::NAN).sidebar_width_for(1400.),
            SIDEBAR_DEFAULT_WIDTH
        );
    }

    #[test]
    fn sidebar_width_leaves_room_for_the_main_pane() {
        assert_eq!(sidebar(400.).sidebar_width_for(620.), 300.);
        assert_eq!(sidebar(400.).sidebar_width_for(300.), SIDEBAR_MIN_WIDTH);
    }

    #[test]
    fn unmoved_split_restores_nothing() {
        assert_eq!(Layout::default().request_height_for(800.), None);
        assert_eq!(split(f32::NAN).request_height_for(800.), None);
        assert_eq!(split(0.3).request_height_for(0.), None);
        assert_eq!(split(0.3).request_height_for(239.), None);
    }

    #[test]
    fn split_scales_to_the_container_and_leaves_both_panes_room() {
        assert_eq!(split(0.25).request_height_for(800.), Some(200.));
        assert_eq!(split(0.01).request_height_for(800.), Some(SPLIT_PANE_MIN));
        assert_eq!(split(1.5).request_height_for(800.), Some(680.));
        assert_eq!(split(0.5).request_height_for(240.), Some(120.));
    }

    #[test]
    fn restored_split_panes_stay_above_the_placeholder_size() {
        for container in [240., 250., 400., 1000.] {
            for share in [0., 0.1, 0.5, 0.9, 1.] {
                let request = split(share).request_height_for(container).unwrap();
                assert!(request > 100. && container - request > 100.);
            }
        }
    }

    #[test]
    fn split_of_pane_sizes() {
        assert_eq!(Layout::split_of(200., 600.), Some(0.25));
        assert_eq!(Layout::split_of(0., 0.), None);
        assert_eq!(Layout::split_of(f32::INFINITY, 1.), None);
    }

    #[test]
    fn layout_round_trips() {
        let layout = Layout {
            sidebar_width: 312.,
            sidebar_collapsed: true,
            request_split: Some(0.4),
        };
        let value = serde_json::to_value(layout).unwrap();
        assert_eq!(serde_json::from_value::<Layout>(value).unwrap(), layout);
    }

    #[test]
    fn missing_or_malformed_fields_keep_their_defaults() {
        let layout: Layout =
            serde_json::from_str(r#"{ "sidebar_width": "wide", "sidebar_collapsed": true }"#)
                .unwrap();
        assert_eq!(
            layout,
            Layout {
                sidebar_collapsed: true,
                ..Layout::default()
            }
        );
        assert_eq!(
            serde_json::from_str::<Layout>("null").unwrap(),
            Layout::default()
        );
    }
}
