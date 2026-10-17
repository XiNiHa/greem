//! The area for graphql-js `src/execution/__tests__/subscribe-test.ts`:
//! its schema, world and resolvers.
//!
//! Upstream splits a subscription field into `subscribe`, which returns the
//! source, and `resolve`, which maps each event; greem's root field resolver
//! returns the event source itself (ADR-0013). The world pre-seeds the
//! source: every event upstream emits or its generator yields, in order, so
//! each resolver returns that list as a stream, or fails before any event
//! when the world says the source cannot be created.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use futures::{Stream, StreamExt, stream};
use greem::{Args, Context, Error, NoMutation, Roots, SchemaBuilder};
use std::sync::{Arc, Mutex};

pub mod schema {
    greem::include_schema!("graphql_js_subscribe.rs");
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Email {
    pub from: &'static str,
    pub subject: &'static str,
    pub message: &'static str,
    pub unread: bool,
}

/// One item of the source stream, per subscription field.
#[derive(Clone, Debug)]
pub enum Event {
    /// What upstream's pubsub emits to `importantEmail`.
    ImportantEmail(Email),
    /// What the `foo` generator yields.
    Foo(&'static str),
    /// What the `newMessage` generator yields.
    NewMessage(&'static str),
}

#[derive(Debug, Default)]
pub struct World {
    pub harness: Harness,
    /// `Query.inbox`'s emails at subscribe time. Each `importantEmail` event
    /// is pushed onto it before its payload is built (upstream's
    /// `emails.push(newEmail)`), so `unread` and `total` count it.
    pub inbox: Arc<Mutex<Vec<Email>>>,
    /// The source stream, pre-seeded: every event upstream emits or its
    /// generator yields, in order. One response per event.
    pub events: Vec<Event>,
    /// The source cannot be created: upstream's `subscribe` returning,
    /// throwing, resolving to or rejecting with this error. The result is
    /// one error response and no event.
    pub source_error: Option<&'static str>,
}

/// The same configuration over a fresh copy of the inbox.
impl Clone for World {
    fn clone(&self) -> Self {
        World {
            harness: self.harness.clone(),
            inbox: Arc::new(Mutex::new(self.inbox.lock().unwrap().clone())),
            events: self.events.clone(),
            source_error: self.source_error,
        }
    }
}

impl World {
    /// Upstream's `createSubscription` data: one read email in the inbox.
    pub fn email_world(events: Vec<Email>) -> Self {
        World {
            harness: Harness::default(),
            inbox: Arc::new(Mutex::new(vec![Email {
                from: "joe@graphql.org",
                subject: "Hello",
                message: "Hello World",
                unread: false,
            }])),
            events: events.into_iter().map(Event::ImportantEmail).collect(),
            source_error: None,
        }
    }

    /// Upstream's `subscribe` step: the events in order, or the error
    /// creating the source raises.
    pub fn source(&self) -> Result<Vec<Event>, Error> {
        match self.source_error {
            Some(message) => Err(Error::new(message)),
            None => Ok(self.events.clone()),
        }
    }
}

impl HasHarness for World {
    fn harness(&self) -> &Harness {
        &self.harness
    }
}

impl Area for World {
    type Info = schema::__private::Info;
    type Query = QueryRoot;
    type Mutation = NoMutation;
    type Subscription = SubscriptionRoot;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation, Self::Subscription>
    {
        schema::Schema::<World>::builder()
            .query::<QueryRoot>()
            .subscription::<SubscriptionRoot>()
    }

    fn roots() -> Roots<QueryRoot, NoMutation, SubscriptionRoot> {
        Roots::query(QueryRoot).with_subscription(SubscriptionRoot)
    }
}

pub type Schema = schema::Schema<World, QueryRoot, NoMutation, SubscriptionRoot>;

pub struct QueryRoot;
/// The subscription root: each field's resolver returns the world's events
/// as its event source.
pub struct SubscriptionRoot;
pub struct Inbox(Arc<Mutex<Vec<Email>>>);
pub struct EmailEvent {
    email: Email,
    inbox: Arc<Mutex<Vec<Email>>>,
}

#[greem::object(schema = crate::graphql_js::subscribe::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn inbox(
        &self,
        _args: &Args<schema::Query::inbox>,
        ctx: &Context<World>,
    ) -> Option<Inbox> {
        ctx.app().harness.trace("Query.inbox", 1).await;
        Some(Inbox(ctx.app().inbox.clone()))
    }
    async fn dummy(
        &self,
        _args: &Args<schema::Query::dummy>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.dummy", 1).await;
        None
    }
}

#[greem::object(schema = crate::graphql_js::subscribe::schema, type = "Subscription", context = World)]
impl SubscriptionRoot {
    async fn important_email(
        &self,
        _args: &Args<schema::Subscription::importantEmail>,
        ctx: &Context<World>,
    ) -> Result<impl Stream<Item = Result<Option<EmailEvent>, Error>> + Send, Error> {
        let app = ctx.app();
        app.harness.trace("Subscription.importantEmail", 1).await;
        let events = app.source()?;
        Ok(stream::iter(events).map(move |event| {
            let Event::ImportantEmail(email) = event else {
                return Ok(None);
            };
            let inbox = app.inbox.clone();
            inbox.lock().unwrap().push(email.clone());
            Ok(Some(EmailEvent { email, inbox }))
        }))
    }
    async fn foo(
        &self,
        _args: &Args<schema::Subscription::foo>,
        ctx: &Context<World>,
    ) -> Result<impl Stream<Item = Result<Option<&str>, Error>> + Send, Error> {
        let app = ctx.app();
        app.harness.trace("Subscription.foo", 1).await;
        let events = app.source()?;
        Ok(stream::iter(events).map(|event| match event {
            Event::Foo(value) => Ok(Some(value)),
            _ => Ok(None),
        }))
    }
    async fn bar(
        &self,
        _args: &Args<schema::Subscription::bar>,
        ctx: &Context<World>,
    ) -> Result<impl Stream<Item = Result<Option<String>, Error>> + Send, Error> {
        let app = ctx.app();
        app.harness.trace("Subscription.bar", 1).await;
        let events = app.source()?;
        Ok(stream::iter(events).map(|_| Ok(None)))
    }
    async fn new_message(
        &self,
        _args: &Args<schema::Subscription::newMessage>,
        ctx: &Context<World>,
    ) -> Result<impl Stream<Item = Result<Option<&str>, Error>> + Send, Error> {
        let app = ctx.app();
        app.harness.trace("Subscription.newMessage", 1).await;
        let events = app.source()?;
        Ok(stream::iter(events).map(|event| match event {
            Event::NewMessage("Goodbye") => Err(Error::new("Never leave.")),
            Event::NewMessage(message) => Ok(Some(message)),
            _ => Ok(None),
        }))
    }
}

#[greem::object(schema = crate::graphql_js::subscribe::schema, type = "EmailEvent", context = World)]
impl EmailEvent {
    async fn email(
        &self,
        _args: &Args<schema::EmailEvent::email>,
        ctx: &Context<World>,
    ) -> Option<&Email> {
        ctx.app().harness.trace("EmailEvent.email", 1).await;
        Some(&self.email)
    }
    async fn inbox(
        &self,
        _args: &Args<schema::EmailEvent::inbox>,
        ctx: &Context<World>,
    ) -> Option<Inbox> {
        ctx.app().harness.trace("EmailEvent.inbox", 1).await;
        Some(Inbox(self.inbox.clone()))
    }
}

#[greem::object(schema = crate::graphql_js::subscribe::schema, type = "Inbox", context = World)]
impl Inbox {
    async fn total(&self, _args: &Args<schema::Inbox::total>, ctx: &Context<World>) -> Option<i32> {
        ctx.app().harness.trace("Inbox.total", 1).await;
        Some(self.0.lock().unwrap().len() as i32)
    }
    async fn unread(
        &self,
        _args: &Args<schema::Inbox::unread>,
        ctx: &Context<World>,
    ) -> Option<i32> {
        ctx.app().harness.trace("Inbox.unread", 1).await;
        Some(self.0.lock().unwrap().iter().filter(|e| e.unread).count() as i32)
    }
    async fn emails(
        &self,
        _args: &Args<schema::Inbox::emails>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<Email>>> {
        ctx.app().harness.trace("Inbox.emails", 1).await;
        Some(self.0.lock().unwrap().iter().cloned().map(Some).collect())
    }
}

#[greem::object(schema = crate::graphql_js::subscribe::schema, type = "Email", context = World)]
impl Email {
    async fn from(&self, _args: &Args<schema::Email::from>, ctx: &Context<World>) -> Option<&str> {
        ctx.app().harness.trace("Email.from", 1).await;
        Some(self.from)
    }
    async fn subject(
        &self,
        _args: &Args<schema::Email::subject>,
        ctx: &Context<World>,
    ) -> Option<&str> {
        ctx.app().harness.trace("Email.subject", 1).await;
        Some(self.subject)
    }
    async fn async_subject(
        &self,
        _args: &Args<schema::Email::asyncSubject>,
        ctx: &Context<World>,
    ) -> Option<&str> {
        ctx.app().harness.trace("Email.asyncSubject", 1).await;
        pending_once().await;
        Some(self.subject)
    }
    async fn message(
        &self,
        _args: &Args<schema::Email::message>,
        ctx: &Context<World>,
    ) -> Option<&str> {
        ctx.app().harness.trace("Email.message", 1).await;
        Some(self.message)
    }
    async fn unread(
        &self,
        _args: &Args<schema::Email::unread>,
        ctx: &Context<World>,
    ) -> Option<bool> {
        ctx.app().harness.trace("Email.unread", 1).await;
        Some(self.unread)
    }
}
