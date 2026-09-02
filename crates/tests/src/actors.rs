use std::{collections::BTreeMap, ops::Index, sync::Arc};

use kival_kernel::create_user;
use reqwest::{Client, cookie::Jar};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    TestKival, TestResult,
    clients::BrowserClient,
    passkeys::{
        InstalledIdentities, PasskeyFixtureError, TEST_ORIGIN, TestPasskey,
        install_test_identities, install_test_identities_for,
    },
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
/// A deterministic user available to integration tests.
pub enum Actor {
    /// The global administrator.
    Admin,
    /// A regular user named Alice.
    Alice,
    /// A regular user named Bob.
    Bob,
    /// A regular user named Charlie.
    Charlie,
    /// A regular user named Dave.
    Dave,
}

impl Actor {
    /// Every actor installed by the fixture.
    pub const ALL: [Self; 5] = [Self::Admin, Self::Alice, Self::Bob, Self::Charlie, Self::Dave];

    /// Returns the actor's login name.
    #[must_use]
    pub const fn username(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Alice => "alice",
            Self::Bob => "bob",
            Self::Charlie => "charlie",
            Self::Dave => "dave",
        }
    }

    /// Returns the actor's human-readable name.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::Alice => "Alice",
            Self::Bob => "Bob",
            Self::Charlie => "Charlie",
            Self::Dave => "Dave",
        }
    }
}

#[derive(Debug, Clone)]
/// An actor together with its authenticated browser client.
pub struct ActorClient {
    /// The actor authenticated by this client.
    pub actor: Actor,
    /// The actor's database user ID.
    pub user_id: Uuid,
    /// Browser-like real-HTTP client holding the actor's session and passkey.
    pub browser: BrowserClient,
}

/// Existing user assigned to a deterministic fixture actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureUser {
    /// Fixture role assigned to the user.
    pub actor: Actor,
    /// Existing database user ID.
    pub user_id: Uuid,
    /// Existing user's canonical login name.
    pub username: String,
}

impl FixtureUser {
    /// Assigns an existing user to a fixture actor.
    #[must_use]
    pub fn new(actor: Actor, user_id: Uuid, username: impl Into<String>) -> Self {
        Self { actor, user_id, username: username.into() }
    }
}

/// Existing users assigned to every fixture actor.
#[derive(Debug, Clone)]
pub struct FixtureUsers {
    /// Users indexed by their assigned fixture actors.
    users: BTreeMap<Actor, FixtureUser>,
}

impl FixtureUsers {
    /// Collects actor assignments for later fixture installation.
    #[must_use]
    pub fn new(users: impl IntoIterator<Item = FixtureUser>) -> Self {
        Self { users: users.into_iter().map(|user| (user.actor, user)).collect() }
    }

    /// Returns the user assigned to `actor`.
    #[must_use]
    pub fn get(&self, actor: Actor) -> Option<&FixtureUser> {
        self.users.get(&actor)
    }
}

impl TestKival {
    /// Provisions four isolated regular users and assigns the bootstrap global
    /// administrator to [`Actor::Admin`].
    ///
    /// # Errors
    ///
    /// Returns an error if a regular user cannot be persisted.
    pub async fn provision_fixture_users(&self) -> TestResult<FixtureUsers> {
        let suffix = Uuid::now_v7().simple().to_string();
        let suffix = &suffix[suffix.len() - 12..];
        let alice =
            self.provision_fixture_user(Actor::Alice, &format!("fx-alice-{suffix}")).await?;
        let bob = self.provision_fixture_user(Actor::Bob, &format!("fx-bob-{suffix}")).await?;
        let charlie =
            self.provision_fixture_user(Actor::Charlie, &format!("fx-charlie-{suffix}")).await?;
        let dave = self.provision_fixture_user(Actor::Dave, &format!("fx-dave-{suffix}")).await?;

        Ok(FixtureUsers::new([
            FixtureUser::new(Actor::Admin, self.admin.id, self.admin.username.clone()),
            alice,
            bob,
            charlie,
            dave,
        ]))
    }

    /// Provisions one regular user through the shared operator persistence primitive.
    async fn provision_fixture_user(
        &self,
        actor: Actor,
        username: &str,
    ) -> TestResult<FixtureUser> {
        let mut transaction = self.pool.begin().await?;
        let created = create_user(&mut transaction, username, actor.display_name()).await?;
        transaction.commit().await?;
        Ok(FixtureUser::new(actor, created.id, created.username))
    }
}

#[derive(Debug, Clone)]
/// Authenticated clients for all fixture actors.
pub struct Actors {
    /// Clients indexed by their corresponding actors.
    clients: BTreeMap<Actor, ActorClient>,
}

impl Actors {
    /// Returns the authenticated client for `actor`.
    #[must_use]
    pub fn get(&self, actor: Actor) -> &ActorClient {
        &self.clients[&actor]
    }

    /// Returns the global administrator's client.
    #[must_use]
    pub fn admin(&self) -> &ActorClient {
        self.get(Actor::Admin)
    }

    /// Returns Alice's client.
    #[must_use]
    pub fn alice(&self) -> &ActorClient {
        self.get(Actor::Alice)
    }

    /// Returns Bob's client.
    #[must_use]
    pub fn bob(&self) -> &ActorClient {
        self.get(Actor::Bob)
    }

    /// Returns Charlie's client.
    #[must_use]
    pub fn charlie(&self) -> &ActorClient {
        self.get(Actor::Charlie)
    }

    /// Returns Dave's client.
    #[must_use]
    pub fn dave(&self) -> &ActorClient {
        self.get(Actor::Dave)
    }

    /// Iterates over actors and their authenticated clients.
    pub fn iter(&self) -> impl Iterator<Item = (&Actor, &ActorClient)> {
        self.clients.iter()
    }
}

impl Index<Actor> for Actors {
    type Output = ActorClient;

    fn index(&self, actor: Actor) -> &Self::Output {
        self.get(actor)
    }
}

#[derive(Debug, Clone)]
/// A complete deterministic identity and authenticated-browser fixture.
pub struct Fixture {
    /// Base URL used for authentication requests.
    pub base_url: String,
    /// Passkey identities installed in the database.
    pub identities: InstalledIdentities,
    /// Authenticated browser clients for the installed identities.
    pub actors: Actors,
}

impl Fixture {
    /// Installs credentials for existing `admin`, `alice`, and `bob` rows and
    /// authenticates one cookie-isolated browser client for each actor.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be installed, an HTTP client or
    /// session cannot be created, or the server authenticates an unexpected user.
    pub async fn install(pool: &PgPool, base_url: impl Into<String>) -> Result<Self, FixtureError> {
        Self::install_with_origin(pool, base_url, TEST_ORIGIN).await
    }

    /// Installs and authenticates the fixture actors using `origin`.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be installed, an HTTP client or
    /// session cannot be created, or the server authenticates an unexpected user.
    pub async fn install_with_origin(
        pool: &PgPool,
        base_url: impl Into<String>,
        origin: &str,
    ) -> Result<Self, FixtureError> {
        let users = FixtureUsers::new(install_test_identities(pool).await?.iter().map(
            |(&actor, identity)| {
                FixtureUser::new(actor, identity.user_id, identity.username.clone())
            },
        ));
        Self::install_for_users(pool, base_url, origin, &users).await
    }

    /// Installs and authenticates explicitly assigned fixture users.
    ///
    /// # Errors
    ///
    /// Returns an error if an actor assignment is missing, credentials cannot
    /// be installed, an HTTP client or session cannot be created, or the server
    /// authenticates an unexpected user.
    pub async fn install_for_users(
        pool: &PgPool,
        base_url: impl Into<String>,
        origin: &str,
        users: &FixtureUsers,
    ) -> Result<Self, FixtureError> {
        let base_url = base_url.into();
        for actor in Actor::ALL {
            if users.get(actor).is_none() {
                return Err(FixtureError::MissingActor(actor));
            }
        }
        let identities = install_test_identities_for(pool, users).await?;
        let mut clients = BTreeMap::new();

        for actor in Actor::ALL {
            let installed = identities.get(actor);
            let cookies = Arc::new(Jar::default());
            let http = Client::builder().cookie_provider(cookies.clone()).build()?;
            let mut passkey = TestPasskey::for_user(actor, installed.user_id)?;

            let session = passkey
                .authenticate_as(&http, &base_url, &installed.username, installed.user_id, origin)
                .await?;

            if session.user.id != installed.user_id || session.user.username != installed.username {
                return Err(FixtureError::UnexpectedAuthenticatedUser {
                    actor,
                    expected_user_id: installed.user_id,
                    actual_user_id: session.user.id,
                    actual_username: session.user.username,
                });
            }

            let browser = BrowserClient::authenticated(
                base_url.clone(),
                origin.to_owned(),
                installed.user_id,
                installed.username.clone(),
                http,
                cookies,
                passkey,
            );
            clients.insert(actor, ActorClient { actor, user_id: installed.user_id, browser });
        }

        Ok(Self { base_url, identities, actors: Actors { clients } })
    }
}

#[derive(Debug, Error)]
/// Failure to install or authenticate a multi-actor fixture.
pub enum FixtureError {
    /// An explicit fixture assignment omitted an actor.
    #[error("fixture user assignment is missing {0:?}")]
    MissingActor(Actor),

    /// Installing or using a deterministic passkey failed.
    #[error(transparent)]
    Passkey(#[from] PasskeyFixtureError),

    /// Constructing or using an HTTP client failed.
    #[error("failed to construct HTTP client")]
    Http(#[from] reqwest::Error),

    /// Authentication succeeded as a user other than the expected actor.
    #[error(
        "{actor:?} authenticated as unexpected user {actual_username} ({actual_user_id}); \
         expected {expected_user_id}"
    )]
    UnexpectedAuthenticatedUser {
        /// Actor whose passkey was used.
        actor: Actor,
        /// User ID associated with the installed passkey.
        expected_user_id: Uuid,
        /// User ID returned by the server.
        actual_user_id: Uuid,
        /// Username returned by the server.
        actual_username: String,
    },
}
