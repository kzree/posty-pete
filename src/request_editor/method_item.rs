use gpui::SharedString;
use gpui_component::searchable_list::SearchableListItem;

use crate::domain::request::HttpMethod;

/// Wraps `HttpMethod` so it can be shown in a `Select` without `domain`
/// depending on gpui-component.
#[derive(Clone)]
pub struct MethodItem(HttpMethod);

impl MethodItem {
    /// One item per HTTP method, in dropdown order.
    pub fn all() -> Vec<MethodItem> {
        HttpMethod::ALL.into_iter().map(MethodItem).collect()
    }
}

impl SearchableListItem for MethodItem {
    type Value = HttpMethod;

    fn title(&self) -> SharedString {
        self.0.as_str().into()
    }

    fn value(&self) -> &HttpMethod {
        &self.0
    }
}
