use crate::clock::{IST_OFFSET_SECONDS, now_unix};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Topic {
    Login,
    Api,
    System,
}

impl Topic {
    const fn label(self) -> &'static str {
        match self {
            Self::Login => "LOGIN",
            Self::Api => "TOKEN API",
            Self::System => "SERVICE",
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::Login => "🔑",
            Self::Api => "🛡",
            Self::System => "🖥",
        }
    }
}

pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[derive(Clone, Debug)]
pub struct Notice {
    problem: bool,
    topic: Topic,
    headline: String,
    facts: Vec<(&'static str, String)>,
    error: Option<String>,
    next: Option<String>,
}

impl Notice {
    pub fn info(topic: Topic, headline: impl Into<String>) -> Self {
        Self {
            problem: false,
            topic,
            headline: headline.into(),
            facts: Vec::new(),
            error: None,
            next: None,
        }
    }

    pub fn problem(topic: Topic, headline: impl Into<String>) -> Self {
        Self {
            problem: true,
            ..Self::info(topic, headline)
        }
    }

    #[must_use]
    pub fn fact(mut self, name: &'static str, value: impl ToString) -> Self {
        self.facts.push((name, value.to_string()));
        self
    }

    #[must_use]
    pub fn error(mut self, detail: impl Into<String>) -> Self {
        self.error = Some(detail.into());
        self
    }

    #[must_use]
    pub fn next(mut self, detail: impl Into<String>) -> Self {
        self.next = Some(detail.into());
        self
    }

    pub fn html(&self) -> String {
        self.html_at(now_unix())
    }

    pub fn html_at(&self, unix: i64) -> String {
        let into_day = (unix + IST_OFFSET_SECONDS).rem_euclid(86_400);
        let clock = format!(
            "{:02}:{:02}:{:02} IST",
            into_day / 3600,
            (into_day % 3600) / 60,
            into_day % 60
        );
        let (icon, label) = if self.problem {
            ("🚨", format!("{} · PROBLEM", self.topic.label()))
        } else {
            (self.topic.icon(), self.topic.label().to_owned())
        };
        let mut lines = vec![
            format!("{icon} <b>{label}</b> · <i>{clock}</i>"),
            format!("<b>{}</b>", escape(&self.headline)),
        ];
        if !self.facts.is_empty() {
            lines.push(String::new());
            lines.extend(
                self.facts
                    .iter()
                    .map(|(name, value)| format!("• {}: <b>{}</b>", escape(name), escape(value))),
            );
        }
        if let Some(error) = &self.error {
            lines.push(String::new());
            lines.push("⚠️ Error".into());
            lines.push(format!(
                "<blockquote expandable>{}</blockquote>",
                escape(error)
            ));
        }
        if let Some(next) = &self.next {
            lines.push(String::new());
            lines.push(format!("➡️ <i>{}</i>", escape(next)));
        }
        lines.join("\n")
    }
}

pub fn visible(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
