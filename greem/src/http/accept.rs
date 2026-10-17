//! `Accept` negotiation: the media type of a single response, and which
//! streaming framing a response may take. Its media type parsing also reads a
//! request's `Content-Type`.

/// The media type of a single (non-incremental) response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Media {
    GraphQLResponseJson,
    /// For legacy clients: only `2xx` responses use it, the rest are sent as
    /// `application/graphql-response+json`.
    Json,
    /// For clients that read no JSON type, only a streaming framing. Reading
    /// only `multipart/mixed`, a `2xx` response goes out as one part and the
    /// rest as `application/graphql-response+json`; reading only
    /// `text/event-stream`, every response, a request error included, is an
    /// event stream at 200, as graphql-sse requires (ADR-0014). A client that
    /// reads both and no JSON is a multipart one, since multipart wins the tie.
    Streaming(Framing),
}

/// A framing that carries several payloads in one response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Framing {
    Multipart,
    EventStream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Negotiated {
    pub media: Media,
    /// The framing for an operation with `@defer` or `@stream`: of the
    /// streaming framings the client reads and prefers at least as much as
    /// `media`, the more preferred, then the more specifically named, then
    /// multipart. `None` disables incremental delivery.
    pub incremental: Option<Framing>,
    /// Whether the client reads `text/event-stream` at all, so a
    /// subscription can stream; its rank does not matter, since nothing
    /// else can carry one.
    pub event_stream: bool,
}

/// How specifically an `Accept` entry names a media type; a more specific
/// entry overrides a less specific one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Specificity {
    Any,
    Subtype,
    Exact,
    /// With parameters that name the format, such as
    /// `multipart/mixed;incrementalSpec=v0.2`.
    Format,
}

#[derive(Clone, Copy, Debug)]
struct Preference {
    q: f32,
    specificity: Specificity,
}

pub(super) struct Entry<'a> {
    ty: &'a str,
    subtype: &'a str,
    q: f32,
    params: Vec<(&'a str, &'a str)>,
}

/// `None` when the client accepts nothing greem can send. A missing or empty
/// `Accept` is `application/json`, as is `*/*`; between equally preferred
/// named types, `application/graphql-response+json` wins, and a streaming
/// framing wins over both.
pub(super) fn negotiate(accept: Option<&str>) -> Option<Negotiated> {
    let Some(accept) = accept.filter(|accept| !accept.trim().is_empty()) else {
        return Some(Negotiated {
            media: Media::Json,
            incremental: None,
            event_stream: false,
        });
    };
    let entries: Vec<Entry> = accept.split(',').filter_map(entry).collect();
    let graphql = preference(&entries, |entry| {
        application(entry, "graphql-response+json")
    });
    let json = preference(&entries, |entry| application(entry, "json"));
    let media = match (graphql, json) {
        (None, None) => None,
        (Some(_), None) => Some(Media::GraphQLResponseJson),
        (None, Some(_)) => Some(Media::Json),
        (Some(graphql), Some(json)) if graphql.q != json.q => Some(if graphql.q > json.q {
            Media::GraphQLResponseJson
        } else {
            Media::Json
        }),
        (Some(graphql), Some(json)) => Some(
            if graphql.specificity > json.specificity
                || (graphql.specificity == json.specificity
                    && graphql.specificity == Specificity::Exact)
            {
                Media::GraphQLResponseJson
            } else {
                Media::Json
            },
        ),
    };
    let sse = preference(&entries, event_stream);
    let streams = |streaming: Option<Preference>| {
        streaming.filter(|streaming| {
            [graphql, json]
                .into_iter()
                .flatten()
                .all(|single| streaming.q >= single.q)
        })
    };
    let incremental = match (
        streams(preference(&entries, incremental_multipart)),
        streams(sse),
    ) {
        (None, None) => None,
        (Some(_), None) => Some(Framing::Multipart),
        (None, Some(_)) => Some(Framing::EventStream),
        (Some(multipart), Some(sse)) => Some(
            if sse.q > multipart.q
                || (sse.q == multipart.q && sse.specificity > multipart.specificity)
            {
                Framing::EventStream
            } else {
                Framing::Multipart
            },
        ),
    };
    let media = match (media, incremental) {
        (Some(media), _) => media,
        (None, Some(framing)) => Media::Streaming(framing),
        (None, None) => return None,
    };
    Some(Negotiated {
        media,
        incremental,
        event_stream: sse.is_some(),
    })
}

pub(super) fn entry(text: &str) -> Option<Entry<'_>> {
    let mut parts = text.split(';').map(str::trim);
    let (ty, subtype) = parts.next()?.split_once('/')?;
    let mut q = 1.0;
    let mut params = Vec::new();
    for param in parts.filter(|param| !param.is_empty()) {
        let (name, value) = param.split_once('=').unwrap_or((param, ""));
        let (name, value) = (name.trim(), value.trim().trim_matches('"'));
        if name.eq_ignore_ascii_case("q") {
            q = value
                .parse::<f32>()
                .ok()
                .filter(|q| (0.0..=1.0).contains(q))?;
        } else {
            params.push((name, value));
        }
    }
    Some(Entry {
        ty: ty.trim(),
        subtype: subtype.trim(),
        q,
        params,
    })
}

/// The preference for the media type that `matches` recognizes, from its
/// most specific matching entries; `None` when nothing matches or that
/// preference is `q=0`.
fn preference(
    entries: &[Entry],
    matches: impl Fn(&Entry) -> Option<Specificity>,
) -> Option<Preference> {
    entries
        .iter()
        .filter_map(|entry| {
            Some(Preference {
                q: entry.q,
                specificity: matches(entry)?,
            })
        })
        .fold(None::<Preference>, |best, next| match best {
            Some(best) if best.specificity > next.specificity => Some(best),
            Some(best) if best.specificity == next.specificity && best.q >= next.q => Some(best),
            _ => Some(next),
        })
        .filter(|preference| preference.q > 0.0)
}

/// How specifically `entry` names `application/<subtype>` in UTF-8.
pub(super) fn application(entry: &Entry, subtype: &str) -> Option<Specificity> {
    match (entry.ty, entry.subtype) {
        ("*", "*") => Some(Specificity::Any),
        (ty, "*") if ty.eq_ignore_ascii_case("application") => Some(Specificity::Subtype),
        (ty, sub)
            if ty.eq_ignore_ascii_case("application")
                && sub.eq_ignore_ascii_case(subtype)
                && entry.params.iter().all(|&(name, value)| {
                    !name.eq_ignore_ascii_case("charset") || value.eq_ignore_ascii_case("utf-8")
                }) =>
        {
            Some(Specificity::Exact)
        }
        _ => None,
    }
}

/// How specifically `entry` names `multipart/mixed` in the format greem
/// serializes: the `pending`/`completed` format that Apollo's incremental
/// delivery v0.2 (<https://specs.apollo.dev/incremental/v0.2>) names
/// `incrementalSpec=v0.2`. A client that names only another format, such as
/// `deferSpec=20220824`, cannot read it, and neither can one assumed to from
/// a wildcard such as `*/*`.
fn incremental_multipart(entry: &Entry) -> Option<Specificity> {
    if !entry.ty.eq_ignore_ascii_case("multipart") || !entry.subtype.eq_ignore_ascii_case("mixed") {
        return None;
    }
    let mut specificity = Specificity::Exact;
    for &(name, value) in &entry.params {
        if name.eq_ignore_ascii_case("deferSpec") {
            return None;
        }
        if name.eq_ignore_ascii_case("incrementalSpec") {
            if value != "v0.2" {
                return None;
            }
            specificity = Specificity::Format;
        }
    }
    Some(specificity)
}

/// How specifically `entry` names `text/event-stream` in UTF-8: by name
/// only, never from a wildcard. graphql-sse defines no parameter naming the
/// incremental format, so any other parameter is ignored.
fn event_stream(entry: &Entry) -> Option<Specificity> {
    (entry.ty.eq_ignore_ascii_case("text")
        && entry.subtype.eq_ignore_ascii_case("event-stream")
        && entry.params.iter().all(|&(name, value)| {
            !name.eq_ignore_ascii_case("charset") || value.eq_ignore_ascii_case("utf-8")
        }))
    .then_some(Specificity::Exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(accept: &str) -> Option<Media> {
        negotiate(Some(accept)).map(|negotiated| negotiated.media)
    }

    #[test]
    fn multipart_is_accepted_only_for_the_serialized_format() {
        let cases = [
            (
                "multipart/mixed;incrementalSpec=v0.2,application/graphql-response+json,application/json;q=0.9",
                true,
            ),
            ("application/json, text/event-stream, multipart/mixed", true),
            ("multipart/mixed", true),
            ("Multipart/Mixed; IncrementalSpec=\"v0.2\"", true),
            (
                "multipart/mixed;deferSpec=20220824, multipart/mixed;incrementalSpec=v0.2",
                true,
            ),
            (super::super::MULTIPART_CONTENT_TYPE, true),
            (
                "multipart/mixed;deferSpec=20220824,application/graphql-response+json,application/json;q=0.9",
                false,
            ),
            ("multipart/mixed;deferSpec=20220824,application/json", false),
            ("multipart/mixed;incrementalSpec=v0.3", false),
            ("multipart/mixed;q=0, application/json", false),
            ("application/json", false),
            ("*/*", false),
        ];
        for (accept, expected) in cases {
            assert_eq!(
                negotiate(Some(accept))
                    .is_some_and(|negotiated| negotiated.incremental == Some(Framing::Multipart)),
                expected,
                "{accept}"
            );
        }
        assert_eq!(negotiate(None).unwrap().incremental, None);
    }

    #[test]
    fn multipart_needs_at_least_the_preference_of_a_single_media_type() {
        let cases = [
            ("multipart/mixed;q=0.1, application/json", false),
            ("multipart/mixed;q=0.5, */*", false),
            (
                "multipart/mixed;q=0.9, application/graphql-response+json",
                false,
            ),
            ("multipart/mixed, application/json;q=0.9", true),
            ("multipart/mixed;q=0.5, application/json;q=0.5", true),
            (
                "multipart/mixed;q=0.5, multipart/mixed;incrementalSpec=v0.2, application/json;q=0.9",
                true,
            ),
            (
                "multipart/mixed;incrementalSpec=v0.2;q=0, multipart/mixed, application/json",
                false,
            ),
        ];
        for (accept, expected) in cases {
            assert_eq!(
                negotiate(Some(accept))
                    .is_some_and(|negotiated| negotiated.incremental == Some(Framing::Multipart)),
                expected,
                "{accept}"
            );
        }
        assert_eq!(
            negotiate(Some("multipart/mixed")),
            Some(Negotiated {
                media: Media::Streaming(Framing::Multipart),
                incremental: Some(Framing::Multipart),
                event_stream: false,
            })
        );
    }

    #[test]
    fn the_single_media_type_follows_preference_then_specificity() {
        use Media::*;
        let cases = [
            ("application/graphql-response+json", GraphQLResponseJson),
            ("application/json", Json),
            (
                "application/graphql-response+json, application/json;q=0.9",
                GraphQLResponseJson,
            ),
            (
                "application/graphql-response+json;q=0.5, application/json",
                Json,
            ),
            (
                "application/json, application/graphql-response+json",
                GraphQLResponseJson,
            ),
            ("*/*", Json),
            ("application/*", Json),
            (
                "application/graphql-response+json, */*",
                GraphQLResponseJson,
            ),
            ("application/json;q=0, */*", GraphQLResponseJson),
            ("application/json; charset=utf-8", Json),
            ("Application/JSON", Json),
        ];
        for (accept, expected) in cases {
            assert_eq!(media(accept), Some(expected), "{accept}");
        }
        assert_eq!(negotiate(None).unwrap().media, Json);
        assert_eq!(negotiate(Some(" ")).unwrap().media, Json);
    }

    #[test]
    fn nothing_greem_can_send_is_not_acceptable() {
        for accept in [
            "text/html",
            "application/json;q=0",
            "application/json; charset=iso-8859-1",
            "application/xml, text/*",
            "multipart/mixed;deferSpec=20220824",
            "multipart/mixed;incrementalSpec=v0.2;q=0, multipart/mixed",
        ] {
            assert_eq!(negotiate(Some(accept)), None, "{accept}");
        }
    }

    #[test]
    fn event_stream_is_accepted_only_by_name() {
        let cases = [
            ("text/event-stream", true),
            ("Text/Event-Stream; charset=\"UTF-8\"", true),
            ("application/json, text/event-stream;q=0.1", true),
            ("text/event-stream;q=0, application/json", false),
            ("text/event-stream; charset=latin1, application/json", false),
            ("text/*, application/json", false),
            ("*/*", false),
            ("application/json", false),
        ];
        for (accept, expected) in cases {
            assert_eq!(
                negotiate(Some(accept)).is_some_and(|negotiated| negotiated.event_stream),
                expected,
                "{accept}"
            );
        }
        assert!(!negotiate(None).unwrap().event_stream);
    }

    #[test]
    fn an_sse_only_client_gets_everything_as_events() {
        assert_eq!(
            negotiate(Some("text/event-stream")),
            Some(Negotiated {
                media: Media::Streaming(Framing::EventStream),
                incremental: Some(Framing::EventStream),
                event_stream: true,
            })
        );
        // Between the two streaming framings alone, multipart wins a tie.
        assert_eq!(
            negotiate(Some("text/event-stream, multipart/mixed")),
            Some(Negotiated {
                media: Media::Streaming(Framing::Multipart),
                incremental: Some(Framing::Multipart),
                event_stream: true,
            })
        );
        assert_eq!(
            negotiate(Some("text/event-stream, multipart/mixed;q=0.9")),
            Some(Negotiated {
                media: Media::Streaming(Framing::EventStream),
                incremental: Some(Framing::EventStream),
                event_stream: true,
            })
        );
    }

    #[test]
    fn incremental_delivery_picks_between_multipart_and_sse() {
        use Framing::*;
        let cases = [
            (
                "text/event-stream, application/json;q=0.9",
                Some(EventStream),
            ),
            ("application/json, text/event-stream", Some(EventStream)),
            ("text/event-stream;q=0.5, application/json", None),
            (
                "multipart/mixed, text/event-stream, application/json;q=0.9",
                Some(Multipart),
            ),
            (
                "text/event-stream, multipart/mixed;q=0.9, application/json;q=0.9",
                Some(EventStream),
            ),
            (
                "multipart/mixed;q=0.9, text/event-stream, application/json",
                Some(EventStream),
            ),
            (
                "text/event-stream;q=0.5, multipart/mixed, application/json",
                Some(Multipart),
            ),
            (
                "multipart/mixed;incrementalSpec=v0.2, text/event-stream, application/json",
                Some(Multipart),
            ),
        ];
        for (accept, expected) in cases {
            assert_eq!(
                negotiate(Some(accept)).unwrap().incremental,
                expected,
                "{accept}"
            );
        }
    }
}
