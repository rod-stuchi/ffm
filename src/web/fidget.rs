//! Telling the page what a long request is doing, step by step — the small
//! translucent box above Save, after neovim's fidget.nvim.
//!
//! The page makes a random job id per Save and sends it as `X-FFM-Job` on each
//! request the Save makes (the trim view's file build, then the upload). Each
//! step goes down the existing `/live` stream (`Live::progress`) tagged with
//! that id and the member: every stream forwards only its own member's steps,
//! and only the page that sent the id shows them. No id, no steps — the houses
//! loader in the button is still there.
//!
//! **Words, never data.** "Reading the bill", "Asking codex to read it" — an
//! amount, a name or a page of the bill never travels here.

use crate::services::live::{Live, Progress};
use axum::http::HeaderMap;

pub struct Fidget {
    live: Option<Live>,
    member: i64,
    job: Option<String>,
}

impl Fidget {
    /// From the request's `X-FFM-Job` header. A missing or malformed one is
    /// simply off.
    pub fn from(headers: &HeaderMap, live: &Live, member: i64) -> Fidget {
        Fidget {
            live: Some(live.clone()),
            member,
            job: headers
                .get("x-ffm-job")
                .and_then(|v| v.to_str().ok())
                .filter(|j| valid(j))
                .map(str::to_string),
        }
    }

    /// For a caller with no request to answer to.
    pub fn off() -> Fidget {
        Fidget {
            live: None,
            member: 0,
            job: None,
        }
    }

    /// The next step has started.
    pub fn step(&self, text: impl Into<String>) {
        if let (Some(live), Some(job)) = (&self.live, &self.job) {
            live.progress(Progress {
                member_id: self.member,
                job: job.clone(),
                text: text.into(),
            });
        }
    }
}

/// 8–64 characters of letters, digits and `-` — a UUID, and nothing that
/// could carry more than an id.
fn valid(job: &str) -> bool {
    (8..=64).contains(&job.len()) && job.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_id_shaped_header_turns_it_on() {
        assert!(valid("4f9c2a1e-1b2c-4d3e-8f00-123456789abc"));
        assert!(!valid("short"));
        assert!(!valid("has spaces in it"));
        assert!(!valid("<script>alert(1)</script>"));
        assert!(!valid(&"a".repeat(65)));

        let (live, _stop) = Live::new();
        let mut rx = live.subscribe();
        let mut h = HeaderMap::new();
        h.insert("x-ffm-job", "job-12345678".parse().unwrap());
        Fidget::from(&h, &live, 7).step("Reading the bill");
        match rx.try_recv() {
            Ok(crate::services::live::Signal::Progress(p)) => {
                assert_eq!(p.member_id, 7);
                assert_eq!(p.job, "job-12345678");
                assert_eq!(
                    p.json(),
                    r#"{"job":"job-12345678","text":"Reading the bill"}"#
                );
            }
            other => panic!("{other:?}"),
        }
        // No header: nothing is sent.
        Fidget::from(&HeaderMap::new(), &live, 7).step("x");
        assert!(rx.try_recv().is_err());
    }
}
