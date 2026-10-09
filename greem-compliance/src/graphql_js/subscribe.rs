//! The area for graphql-js v17.0.2 `src/execution/__tests__/subscribe-test.ts`:
//! its schema, world and resolvers.
//!
//! greem executes no subscription yet (#40): `Schema::execute` rejects the
//! operation at prepare and `Roots` has no subscription root. The world
//! therefore holds the source as a pre-seeded list of events, and
//! [`SubscriptionRoot`] is the root value one event is executed with, the way
//! upstream maps each source item to the root value of
//! `executeSubscriptionEvent`. The cases that subscribe are written against
//! that shape and ignored until #40 lands.

use crate::harness::{Area, Harness, HasHarness, pending_once};
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

    /// Upstream's `subscribe` step: the root value of every event the
    /// subscription executes, or the error creating the source raises.
    pub fn source(&self) -> Result<Vec<SubscriptionRoot>, Error> {
        match self.source_error {
            Some(message) => Err(Error::new(message)),
            None => Ok(self.events.iter().cloned().map(SubscriptionRoot).collect()),
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

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<QueryRoot>()
    }

    fn roots() -> Roots<QueryRoot, NoMutation> {
        Roots::query(QueryRoot)
    }
}

pub type Schema = schema::Schema<World, QueryRoot, NoMutation>;

pub struct QueryRoot;
/// The root value one source event is executed with.
pub struct SubscriptionRoot(pub Event);
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
    ) -> Option<EmailEvent> {
        ctx.app()
            .harness
            .trace("Subscription.importantEmail", 1)
            .await;
        let Event::ImportantEmail(email) = &self.0 else {
            return None;
        };
        let inbox = ctx.app().inbox.clone();
        inbox.lock().unwrap().push(email.clone());
        Some(EmailEvent {
            email: email.clone(),
            inbox,
        })
    }
    async fn foo(
        &self,
        _args: &Args<schema::Subscription::foo>,
        ctx: &Context<World>,
    ) -> Option<&str> {
        ctx.app().harness.trace("Subscription.foo", 1).await;
        match self.0 {
            Event::Foo(value) => Some(value),
            _ => None,
        }
    }
    async fn bar(
        &self,
        _args: &Args<schema::Subscription::bar>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Subscription.bar", 1).await;
        None
    }
    async fn new_message(
        &self,
        _args: &Args<schema::Subscription::newMessage>,
        ctx: &Context<World>,
    ) -> Result<Option<&str>, Error> {
        ctx.app().harness.trace("Subscription.newMessage", 1).await;
        match self.0 {
            Event::NewMessage("Goodbye") => Err(Error::new("Never leave.")),
            Event::NewMessage(message) => Ok(Some(message)),
            _ => Ok(None),
        }
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
