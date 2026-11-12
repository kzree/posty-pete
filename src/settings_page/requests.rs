use std::ops::RangeInclusive;

use domain::{
    http,
    settings::{ProxyMode, RequestSettings},
    settings_index::{Setting, SettingsGroup},
};
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, prelude::FluentBuilder, px,
};
use gpui_component::{
    Disableable,
    input::{Input, InputEvent, InputState, NumberInput},
    radio::RadioGroup,
    switch::Switch,
};

use super::{Reveal, group, number_state, page, row, row_standing_in};
use crate::settings::AppSettings;

const PROXY_MODES: [ProxyMode; 3] = [ProxyMode::None, ProxyMode::System, ProxyMode::Custom];

fn proxy_label(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::None => "None",
        ProxyMode::System => "System",
        ProxyMode::Custom => "Custom",
    }
}

/// A typed whole number, clamped to `range`. `None` while it isn't a number.
fn parse_number(text: &str, range: &RangeInclusive<u32>) -> Option<u32> {
    let value = text.trim().parse::<u64>().ok()?;
    Some(value.clamp((*range.start()).into(), (*range.end()).into()) as u32)
}

fn requests(cx: &App) -> &RequestSettings {
    &AppSettings::get(cx).requests
}

pub struct RequestsPage {
    reveal: Entity<Reveal>,
    timeout: Entity<InputState>,
    max_redirects: Entity<InputState>,
    proxy_url: Entity<InputState>,
    user_agent: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl RequestsPage {
    pub fn new(reveal: Entity<Reveal>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = requests(cx).clone();
        let timeout = RequestSettings::TIMEOUT_SECS;
        let redirects = RequestSettings::MAX_REDIRECTS;
        let timeout = cx.new(|cx| {
            number_state(
                settings.timeout_secs,
                *timeout.start(),
                *timeout.end(),
                window,
                cx,
            )
        });
        let max_redirects = cx.new(|cx| {
            number_state(
                settings.max_redirects,
                *redirects.start(),
                *redirects.end(),
                window,
                cx,
            )
        });
        let proxy_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://proxy.example.com:8080")
                .default_value(settings.proxy_url)
        });
        // reqwest sends no User-Agent unless one is set.
        let user_agent = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("None")
                .default_value(settings.user_agent)
        });
        let _subscriptions = vec![
            on_change(&timeout, cx, |s, text| {
                if let Some(secs) = parse_number(text, &RequestSettings::TIMEOUT_SECS) {
                    s.timeout_secs = secs;
                }
            }),
            on_change(&max_redirects, cx, |s, text| {
                if let Some(max) = parse_number(text, &RequestSettings::MAX_REDIRECTS) {
                    s.max_redirects = max;
                }
            }),
            on_change(&proxy_url, cx, |s, text| s.proxy_url = text.to_owned()),
            on_change(&user_agent, cx, |s, text| s.user_agent = text.to_owned()),
            cx.observe_global::<AppSettings>(|_, cx| cx.notify()),
        ];
        Self {
            reveal,
            timeout,
            max_redirects,
            proxy_url,
            user_agent,
            _subscriptions,
        }
    }

    fn render_proxy(&self, cx: &App) -> RadioGroup {
        let current = requests(cx).proxy;
        RadioGroup::horizontal("proxy")
            .children(PROXY_MODES.map(proxy_label))
            .selected_index(PROXY_MODES.iter().position(|m| *m == current))
            .on_change(|index: &usize, _, cx| {
                let mode = PROXY_MODES[*index];
                AppSettings::update(cx, |s| s.requests.proxy = mode);
            })
    }

    fn render_connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = &self.reveal;
        let settings = requests(cx);
        let verify_tls = settings.verify_tls;
        let custom_proxy = settings.proxy == ProxyMode::Custom;
        let proxy_error = custom_proxy
            .then(|| http::parse_proxy_url(&settings.proxy_url).err())
            .flatten();
        group(SettingsGroup::Connection)
            .child(
                row(reveal, Setting::Timeout, cx)
                    .description("In seconds. 0 means no timeout.")
                    .control(
                        NumberInput::new(&self.timeout)
                            .accessibility_label("Timeout in seconds")
                            .w(px(120.)),
                    ),
            )
            .child(
                row(reveal, Setting::VerifyTls, cx)
                    .when(!verify_tls, |row| {
                        row.warning(
                            "Anyone on the network could read or change requests to HTTPS servers.",
                        )
                    })
                    .control(switch(Setting::VerifyTls, verify_tls, |s, on| {
                        s.verify_tls = on
                    })),
            )
            .child(
                // Proxy URL shows only for a custom proxy.
                row_standing_in(
                    reveal,
                    Setting::Proxy,
                    (!custom_proxy).then_some(Setting::ProxyUrl),
                    cx,
                )
                .description(
                    "System uses the HTTP_PROXY, HTTPS_PROXY and NO_PROXY environment variables, \
                     which are usually only set when the app is opened from a terminal.",
                )
                .control(self.render_proxy(cx)),
            )
            .when(custom_proxy, |group| {
                group.child(
                    row(reveal, Setting::ProxyUrl, cx)
                        .when_some(proxy_error, |row, error| row.error(error.to_string()))
                        .control(
                            Input::new(&self.proxy_url)
                                .aria_label("Proxy URL")
                                .w(px(240.)),
                        ),
                )
            })
    }

    fn render_redirects(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = &self.reveal;
        let follow_redirects = requests(cx).follow_redirects;
        group(SettingsGroup::Redirects)
            .child(
                row(reveal, Setting::FollowRedirects, cx)
                    .description(
                        "Only to the same origin; other redirects are shown as the response.",
                    )
                    .control(switch(
                        Setting::FollowRedirects,
                        follow_redirects,
                        |s, on| s.follow_redirects = on,
                    )),
            )
            .child(
                row(reveal, Setting::MaxRedirects, cx).control(
                    NumberInput::new(&self.max_redirects)
                        .accessibility_label("Maximum redirects")
                        .disabled(!follow_redirects)
                        .w(px(120.)),
                ),
            )
    }

    fn render_headers(&self, cx: &App) -> impl IntoElement {
        let error = http::parse_user_agent(&requests(cx).user_agent).err();
        group(SettingsGroup::Headers).child(
            row(&self.reveal, Setting::UserAgent, cx)
                .description("Sent when a request doesn't set its own.")
                .when_some(error, |row, error| row.error(error.to_string()))
                .control(
                    Input::new(&self.user_agent)
                        .aria_label("User-Agent")
                        .w(px(240.)),
                ),
        )
    }
}

/// Applies `set` to the request settings with the input's text on every edit.
fn on_change(
    input: &Entity<InputState>,
    cx: &mut Context<RequestsPage>,
    set: fn(&mut RequestSettings, &str),
) -> Subscription {
    cx.subscribe(input, move |_, input, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
            let text = input.read(cx).value();
            AppSettings::update(cx, |s| set(&mut s.requests, &text));
        }
    })
}

/// A switch for a request setting, named after `setting`.
fn switch(setting: Setting, checked: bool, set: fn(&mut RequestSettings, bool)) -> Switch {
    Switch::new(setting.id())
        .checked(checked)
        .accessibility_label(setting.label())
        .on_change(move |checked: &bool, _, cx| {
            let checked = *checked;
            AppSettings::update(cx, |s| set(&mut s.requests, checked));
        })
}

impl Render for RequestsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        page()
            .child(self.render_connection(cx))
            .child(self.render_redirects(cx))
            .child(self.render_headers(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_numbers_are_clamped_and_blanks_ignored() {
        let range = RequestSettings::MAX_REDIRECTS;
        assert_eq!(parse_number(" 7 ", &range), Some(7));
        assert_eq!(parse_number("0", &range), Some(1));
        assert_eq!(parse_number("999", &range), Some(50));
        assert_eq!(parse_number("", &range), None);
        assert_eq!(parse_number("99999999999", &range), Some(50));
        assert_eq!(parse_number("-1", &range), None);
    }
}
