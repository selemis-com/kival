//! Passkey enrollment-code and reset scenario tests.

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{
            HeaderValue, Method, StatusCode,
            header::{CACHE_CONTROL, SET_COOKIE},
        },
        response::Response,
    };
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use chrono::{DateTime, Utc};
    use ciborium::Value as CborValue;
    use eyre::Result;
    use kival_common::security;
    use kival_sdk::ApiErrorResponse;
    use kival_tests::{TestActor, TestFixtureExt, TestKival, TestResponseExt};
    use ring::{
        digest,
        rand::{SecureRandom, SystemRandom},
        signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair},
    };
    use serde_json::{Value, json};
    use tokio::time::{Duration, timeout};
    use uuid::Uuid;

    /// Marks the bootstrap admin session as recently passkey-authenticated.
    async fn mark_admin_fresh(kival: &TestKival) -> Result<()> {
        sqlx::query(
            "UPDATE kival.sessions SET fresh_authenticated_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(kival.admin.id)
        .execute(&kival.pool)
        .await?;
        Ok(())
    }

    /// Inserts the same hashed, operator-attributed capability produced by `kivald`.
    async fn issue_operator_enrollment_code(
        kival: &TestKival,
        user_id: Uuid,
        purpose: &str,
    ) -> Result<String> {
        let code = format!("kvl_enroll_{}", security::generate_secret_token()?);
        let code_hash = security::hash_token(&code);
        sqlx::query(
            r#"
            INSERT INTO kival.passkey_enrollment_codes (
                user_id, code_hash, created_by, created_by_operator, purpose, expires_at
            )
            VALUES ($1, $2, NULL, true, $3, now() + interval '30 minutes')
            "#,
        )
        .bind(user_id)
        .bind(code_hash.as_slice())
        .bind(purpose)
        .execute(&kival.pool)
        .await?;
        Ok(code)
    }

    fn assert_private_no_store(response: &Response) {
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CACHE_CONTROL).and_then(|value| value.to_str().ok()),
            Some("private, no-store")
        );
    }

    fn rotated_actor(response: &Response, source: &TestActor) -> Result<TestActor> {
        let cookies = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>();
        for cookie in &cookies {
            if cookie.starts_with("__Host-") {
                assert!(cookie.contains("; Path=/"));
                assert!(cookie.contains("; Secure"));
                assert!(!cookie.contains("Domain="));
            }
        }
        let session = cookies
            .iter()
            .find_map(|cookie| cookie.strip_prefix("__Host-kival_session="))
            .and_then(|value| value.split(';').next())
            .expect("fresh authentication must rotate the session cookie");
        let csrf = cookies
            .iter()
            .find_map(|cookie| cookie.strip_prefix("__Host-kival_csrf="))
            .and_then(|value| value.split(';').next())
            .expect("fresh authentication must rotate the CSRF cookie");

        Ok(TestActor {
            id: source.id,
            username: source.username.clone(),
            cookie_header: HeaderValue::from_str(&format!(
                "__Host-kival_session={session}; __Host-kival_csrf={csrf}"
            ))?,
            csrf_token: HeaderValue::from_str(csrf)?,
        })
    }

    struct TestPasskey {
        credential_id: [u8; 32],
        key_pair: EcdsaKeyPair,
    }

    impl TestPasskey {
        fn new() -> Self {
            let random = SystemRandom::new();
            let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random)
                .expect("test key generation must succeed");
            let key_pair = EcdsaKeyPair::from_pkcs8(
                &ECDSA_P256_SHA256_ASN1_SIGNING,
                document.as_ref(),
                &random,
            )
            .expect("test key must parse");
            let mut credential_id = [0_u8; 32];
            random.fill(&mut credential_id).expect("test randomness must succeed");
            Self { credential_id, key_pair }
        }

        async fn install(&self, kival: &TestKival, user_id: Uuid) -> Result<()> {
            sqlx::query(
                r#"
                INSERT INTO kival.passkey_credentials
                    (user_id, credential_id, public_key)
                VALUES ($1, $2, $3)
                "#,
            )
            .bind(user_id)
            .bind(self.credential_id.as_slice())
            .bind(self.key_pair.public_key().as_ref())
            .execute(&kival.pool)
            .await?;
            Ok(())
        }

        fn assertion(
            &self,
            options: &Value,
            user_handle: Option<Uuid>,
            signature_count: u32,
        ) -> Value {
            let challenge = options["publicKey"]["challenge"]
                .as_str()
                .expect("authentication options must contain a challenge");
            let client_data = serde_json::to_vec(&json!({
                "type": "webauthn.get",
                "challenge": challenge,
                "origin": "http://localhost:5173",
                "crossOrigin": false
            }))
            .expect("test client data must serialize");
            let mut authenticator_data =
                digest::digest(&digest::SHA256, b"localhost").as_ref().to_vec();
            authenticator_data.push(0x01 | 0x04);
            authenticator_data.extend_from_slice(&signature_count.to_be_bytes());

            let client_hash = digest::digest(&digest::SHA256, &client_data);
            let mut signed = authenticator_data.clone();
            signed.extend_from_slice(client_hash.as_ref());
            let signature = self
                .key_pair
                .sign(&SystemRandom::new(), &signed)
                .expect("test assertion signing must succeed");
            let credential_id = URL_SAFE_NO_PAD.encode(self.credential_id);

            json!({
                "ceremonyId": options["ceremonyId"],
                "credential": {
                    "id": credential_id,
                    "rawId": credential_id,
                    "type": "public-key",
                    "response": {
                        "authenticatorData": URL_SAFE_NO_PAD.encode(authenticator_data),
                        "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
                        "signature": URL_SAFE_NO_PAD.encode(signature.as_ref()),
                        "userHandle": user_handle.map(|id| URL_SAFE_NO_PAD.encode(id.as_bytes()))
                    }
                }
            })
        }

        fn registration(&self, options: &Value) -> Value {
            let challenge = options["publicKey"]["challenge"]
                .as_str()
                .expect("registration options must contain a challenge");
            let client_data = serde_json::to_vec(&json!({
                "type": "webauthn.create",
                "challenge": challenge,
                "origin": "http://localhost:5173",
                "crossOrigin": false
            }))
            .expect("test client data must serialize");

            let public_key = self.key_pair.public_key().as_ref();
            let cose = CborValue::Map(vec![
                (CborValue::Integer(1.into()), CborValue::Integer(2.into())),
                (CborValue::Integer(3.into()), CborValue::Integer((-7).into())),
                (CborValue::Integer((-1).into()), CborValue::Integer(1.into())),
                (CborValue::Integer((-2).into()), CborValue::Bytes(public_key[1..33].to_vec())),
                (CborValue::Integer((-3).into()), CborValue::Bytes(public_key[33..65].to_vec())),
            ]);
            let mut cose_bytes = Vec::new();
            ciborium::ser::into_writer(&cose, &mut cose_bytes)
                .expect("test COSE key must serialize");

            let mut authenticator_data =
                digest::digest(&digest::SHA256, b"localhost").as_ref().to_vec();
            authenticator_data.push(0x01 | 0x04 | 0x40);
            authenticator_data.extend_from_slice(&0_u32.to_be_bytes());
            authenticator_data.extend_from_slice(&[0_u8; 16]);
            authenticator_data.extend_from_slice(
                &u16::try_from(self.credential_id.len())
                    .expect("test credential ID length must fit")
                    .to_be_bytes(),
            );
            authenticator_data.extend_from_slice(&self.credential_id);
            authenticator_data.extend_from_slice(&cose_bytes);

            let attestation = CborValue::Map(vec![
                (CborValue::Text("fmt".to_owned()), CborValue::Text("none".to_owned())),
                (CborValue::Text("attStmt".to_owned()), CborValue::Map(Vec::new())),
                (CborValue::Text("authData".to_owned()), CborValue::Bytes(authenticator_data)),
            ]);
            let mut attestation_bytes = Vec::new();
            ciborium::ser::into_writer(&attestation, &mut attestation_bytes)
                .expect("test attestation object must serialize");
            let credential_id = URL_SAFE_NO_PAD.encode(self.credential_id);

            json!({
                "id": credential_id,
                "rawId": credential_id,
                "type": "public-key",
                "response": {
                    "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
                    "attestationObject": URL_SAFE_NO_PAD.encode(attestation_bytes)
                }
            })
        }
    }

    async fn start_login(kival: &TestKival, username: &str) -> Result<Value> {
        kival
            .request_json::<Value>(
                None,
                Method::POST,
                "/auth/passkey/authentication/options",
                Some(json!({ "username": username })),
            )
            .await?
            .into_success()
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn authentication_resolves_usernames_case_insensitively(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("case-sensitive-display").await?;
        let key = TestPasskey::new();
        key.install(&kival, user.id).await?;

        let options = start_login(&kival, &user.username.to_uppercase()).await?;
        let response = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/finish",
                Some(key.assertion(&options, Some(user.id), 1)),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn ceremony_options_are_private_and_not_stored(pool: sqlx::PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        mark_admin_fresh(&kival).await?;
        TestPasskey::new().install(&kival, kival.admin.id).await?;

        let authentication = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/options",
                Some(json!({ "username": &kival.admin.username })),
            )
            .await?;
        assert_private_no_store(&authentication);

        let registration = kival
            .request(Some(&kival.admin), Method::POST, "/auth/passkeys/registration/options", None)
            .await?;
        assert_private_no_store(&registration);

        let fresh = kival
            .request(Some(&kival.admin), Method::POST, "/auth/passkeys/fresh/options", None)
            .await?;
        assert_private_no_store(&fresh);

        let target = kival.create_user("passkey-no-store-enrollment").await?;
        let code = issue_operator_enrollment_code(&kival, target.id, "enrollment").await?;
        let enrollment = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/enrollment/options",
                Some(json!({ "username": target.username, "code": code })),
            )
            .await?;
        assert_private_no_store(&enrollment);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn passkey_metadata_uses_rfc3339_timestamps(pool: sqlx::PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        TestPasskey::new().install(&kival, kival.admin.id).await?;

        let response = kival
            .request_json::<Value>(Some(&kival.admin), Method::GET, "/auth/passkeys", None)
            .await?
            .into_success()?;
        let passkey = response["items"].as_array().and_then(|items| items.first()).unwrap();

        for field in ["createdAt", "updatedAt"] {
            let value = passkey[field].as_str().expect("timestamp must be a string");
            value.parse::<DateTime<Utc>>()?;
        }
        assert!(
            passkey["lastUsedAt"].is_null() || passkey["lastUsedAt"].as_str().is_some(),
            "optional timestamps must be null or strings"
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn zero_passkey_session_still_requires_fresh_authentication_for_registration(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        sqlx::query(
            "UPDATE kival.sessions SET fresh_authenticated_at = NULL WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(kival.admin.id)
        .execute(&kival.pool)
        .await?;

        let active_passkeys = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.passkey_credentials WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(kival.admin.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(active_passkeys, 0);

        let response = kival
            .request(Some(&kival.admin), Method::POST, "/auth/passkeys/registration/options", None)
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn fresh_authentication_rotates_session_and_invalidates_cloned_credentials(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let key = TestPasskey::new();
        key.install(&kival, kival.admin.id).await?;
        let cloned_session = kival.admin.clone();

        let options = kival
            .request_json::<Value>(
                Some(&kival.admin),
                Method::POST,
                "/auth/passkeys/fresh/options",
                None,
            )
            .await?
            .into_success()?;
        let assertion = key.assertion(&options, Some(kival.admin.id), 1);
        let response = kival
            .request(
                Some(&kival.admin),
                Method::POST,
                "/auth/passkeys/fresh/finish",
                Some(assertion),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let rotated = rotated_actor(&response, &kival.admin)?;

        let cloned_response =
            kival.request(Some(&cloned_session), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(cloned_response.status(), StatusCode::UNAUTHORIZED);

        let rotated_response =
            kival.request(Some(&rotated), Method::GET, "/auth/whoami", None).await?;
        assert_eq!(rotated_response.status(), StatusCode::OK);

        let cloned_registration = kival
            .request(
                Some(&cloned_session),
                Method::POST,
                "/auth/passkeys/registration/options",
                None,
            )
            .await?;
        assert_eq!(cloned_registration.status(), StatusCode::UNAUTHORIZED);

        let rotated_registration = kival
            .request(Some(&rotated), Method::POST, "/auth/passkeys/registration/options", None)
            .await?;
        assert_eq!(rotated_registration.status(), StatusCode::OK);

        let active_sessions = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.sessions WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now()",
        )
        .bind(kival.admin.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(active_sessions, 1);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn login_start_uses_indistinguishable_ceremony_id_formats(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let with_passkey = kival.create_user("pk-login-format-key").await?;
        let without_passkey = kival.create_user("pk-login-format-no-key").await?;
        let key = TestPasskey::new();
        key.install(&kival, with_passkey.id).await?;

        let states = [
            start_login(&kival, &with_passkey.username).await?,
            start_login(&kival, &without_passkey.username).await?,
            start_login(&kival, "missing-passkey-user").await?,
        ];

        for options in states {
            let ceremony_id = Uuid::parse_str(
                options["ceremonyId"].as_str().expect("ceremony ID must be a string"),
            )?;
            assert_eq!(ceremony_id.get_version_num(), 4);
        }

        let stored_version = sqlx::query_scalar::<_, i16>(
            "SELECT uuid_extract_version(id) FROM kival.webauthn_ceremonies WHERE user_id = $1",
        )
        .bind(with_passkey.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(stored_version, 4);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn authentication_finish_does_not_disclose_account_state(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let with_passkey = kival.create_user("pk-finish-oracle-key").await?;
        let without_passkey = kival.create_user("pk-finish-oracle-no-key").await?;
        let installed = TestPasskey::new();
        installed.install(&kival, with_passkey.id).await?;
        let bogus = TestPasskey::new();

        let states = [
            start_login(&kival, &with_passkey.username).await?,
            start_login(&kival, &without_passkey.username).await?,
            start_login(&kival, "missing-passkey-finish").await?,
        ];
        let mut failures = Vec::new();

        for options in states {
            let response = kival
                .request(
                    None,
                    Method::POST,
                    "/auth/passkey/authentication/finish",
                    Some(bogus.assertion(&options, Some(with_passkey.id), 1)),
                )
                .await?;
            let status = response.status();
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            let error: ApiErrorResponse = serde_json::from_slice(&body)?;
            failures.push((status, error.error.code, error.error.message));
        }

        assert!(failures.iter().all(|failure| failure == &failures[0]));
        assert_eq!(
            failures[0],
            (
                StatusCode::UNAUTHORIZED,
                "unauthorized".to_owned(),
                "authentication failed".to_owned(),
            ),
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn login_start_prunes_terminal_ceremonies(pool: sqlx::PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("passkey-login-pruning").await?;
        let key = TestPasskey::new();
        key.install(&kival, user.id).await?;

        sqlx::query(
            r#"
            INSERT INTO kival.webauthn_ceremonies
                (user_id, kind, challenge, created_at, expires_at)
            VALUES (
                $1, 'authentication', $2,
                now() - interval '10 minutes', now() - interval '5 minutes'
            )
            "#,
        )
        .bind(user.id)
        .bind(vec![0_u8; 32])
        .execute(&kival.pool)
        .await?;

        start_login(&kival, &user.username).await?;

        let expired = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM kival.webauthn_ceremonies
            WHERE user_id = $1
                AND expires_at <= now()
            "#,
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(expired, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn login_is_bound_to_the_identifier_resolved_user(pool: sqlx::PgPool) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let first = kival.create_user("passkey-binding-first").await?;
        let second = kival.create_user("passkey-binding-second").await?;
        let first_key = TestPasskey::new();
        let second_key = TestPasskey::new();
        first_key.install(&kival, first.id).await?;
        second_key.install(&kival, second.id).await?;

        let options = start_login(&kival, &first.username).await?;
        assert_eq!(options["publicKey"]["allowCredentials"], json!([]));
        assert_eq!(options["publicKey"]["userVerification"], "required");

        let other_credential = second_key.assertion(&options, Some(second.id), 1);
        let response = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/finish",
                Some(other_credential),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let mut missing_handle = first_key.assertion(&options, None, 1);
        missing_handle["credential"]["response"]
            .as_object_mut()
            .expect("credential response must be an object")
            .remove("userHandle");
        let response = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/finish",
                Some(missing_handle),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let null_handle = first_key.assertion(&options, None, 1);
        let response = kival
            .request(None, Method::POST, "/auth/passkey/authentication/finish", Some(null_handle))
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let wrong_handle = first_key.assertion(&options, Some(second.id), 1);
        let response = kival
            .request(None, Method::POST, "/auth/passkey/authentication/finish", Some(wrong_handle))
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let valid = first_key.assertion(&options, Some(first.id), 1);
        let response = kival
            .request(None, Method::POST, "/auth/passkey/authentication/finish", Some(valid))
            .await?;
        assert_eq!(response.status(), StatusCode::OK);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn login_ceremonies_reject_replay_expiry_and_wrong_kind(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("passkey-ceremony-lifecycle").await?;
        let key = TestPasskey::new();
        key.install(&kival, user.id).await?;

        let options = start_login(&kival, &user.username).await?;
        let assertion = key.assertion(&options, Some(user.id), 1);
        let first = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/finish",
                Some(assertion.clone()),
            )
            .await?;
        assert_eq!(first.status(), StatusCode::OK);
        let replay = kival
            .request(None, Method::POST, "/auth/passkey/authentication/finish", Some(assertion))
            .await?;
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

        let expired_challenge = [6_u8; 32];
        let expired_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.webauthn_ceremonies
                (user_id, kind, challenge, created_at, expires_at)
            VALUES ($1, 'authentication', $2, now() - interval '10 minutes',
                    now() - interval '5 minutes')
            RETURNING id
            "#,
        )
        .bind(user.id)
        .bind(expired_challenge.as_slice())
        .fetch_one(&kival.pool)
        .await?;
        let expired_options = json!({
            "ceremonyId": expired_id,
            "publicKey": { "challenge": URL_SAFE_NO_PAD.encode(expired_challenge) }
        });
        let expired = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/finish",
                Some(key.assertion(&expired_options, Some(user.id), 2)),
            )
            .await?;
        assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);

        let session_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM kival.sessions WHERE user_id = $1 AND revoked_at IS NULL LIMIT 1",
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        let wrong_kind_challenge = [7_u8; 32];
        let wrong_kind_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.webauthn_ceremonies
                (user_id, session_id, kind, challenge, expires_at)
            VALUES ($1, $2, 'registration', $3, now() + interval '5 minutes')
            RETURNING id
            "#,
        )
        .bind(user.id)
        .bind(session_id)
        .bind(wrong_kind_challenge.as_slice())
        .fetch_one(&kival.pool)
        .await?;
        let wrong_kind_options = json!({
            "ceremonyId": wrong_kind_id,
            "publicKey": { "challenge": URL_SAFE_NO_PAD.encode(wrong_kind_challenge) }
        });
        let wrong_kind = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/authentication/finish",
                Some(key.assertion(&wrong_kind_options, Some(user.id), 2)),
            )
            .await?;
        assert_eq!(wrong_kind.status(), StatusCode::UNAUTHORIZED);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn concurrent_login_completion_consumes_the_ceremony_once(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("passkey-concurrent-login").await?;
        let key = TestPasskey::new();
        key.install(&kival, user.id).await?;
        let options = start_login(&kival, &user.username).await?;
        let assertion = key.assertion(&options, Some(user.id), 1);

        let first = kival.request(
            None,
            Method::POST,
            "/auth/passkey/authentication/finish",
            Some(assertion.clone()),
        );
        let second = kival.request(
            None,
            Method::POST,
            "/auth/passkey/authentication/finish",
            Some(assertion),
        );
        let (first, second) = tokio::join!(first, second);
        let mut statuses = [first?.status(), second?.status()];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::OK, StatusCode::UNAUTHORIZED]);

        let sessions = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.sessions WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(sessions, 2, "fixture session plus one passkey login session");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn committed_recovery_revocation_wins_against_in_progress_login(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("passkey-recovery-race").await?;
        let key = TestPasskey::new();
        key.install(&kival, user.id).await?;
        let options = start_login(&kival, &user.username).await?;
        let assertion = key.assertion(&options, Some(user.id), 1);

        let mut recovery = kival.pool.begin().await?;
        sqlx::query_scalar::<_, i32>("SELECT 1 FROM kival.users WHERE id = $1 FOR UPDATE")
            .bind(user.id)
            .fetch_one(&mut *recovery)
            .await?;

        let finish = kival.request(
            None,
            Method::POST,
            "/auth/passkey/authentication/finish",
            Some(assertion),
        );
        tokio::pin!(finish);
        assert!(
            timeout(Duration::from_millis(100), &mut finish).await.is_err(),
            "login completion must wait for the recovery user lock"
        );

        sqlx::query(
            r#"
            UPDATE kival.passkey_credentials
            SET revoked_at = now(), revoked_by = NULL, revoked_by_operator = true,
                revocation_reason = 'deployment_operator_recovery'
            WHERE user_id = $1
                AND revoked_at IS NULL
            "#,
        )
        .bind(user.id)
        .execute(&mut *recovery)
        .await?;
        sqlx::query(
            r#"
            UPDATE kival.sessions
            SET revoked_at = now(), revoked_by = NULL, revoked_by_operator = true,
                revocation_reason = 'deployment_operator_recovery'
            WHERE user_id = $1
                AND revoked_at IS NULL
            "#,
        )
        .bind(user.id)
        .execute(&mut *recovery)
        .await?;
        recovery.commit().await?;

        let response = finish.await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let active_sessions = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.sessions WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(active_sessions, 0);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn operator_enrollment_code_is_redeemable_only_for_its_username(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let target = kival.create_user("passkey-enrollment-target").await?;
        let code = issue_operator_enrollment_code(&kival, target.id, "enrollment").await?;
        assert!(code.starts_with("kvl_enroll_"));

        let stored_hash = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT code_hash FROM kival.passkey_enrollment_codes WHERE user_id = $1",
        )
        .bind(target.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(stored_hash, security::hash_token(&code).as_slice());

        let wrong_username = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/enrollment/options",
                Some(json!({ "username": "wrong-username", "code": code })),
            )
            .await?;
        assert_eq!(wrong_username.status(), StatusCode::UNAUTHORIZED);

        let options = kival
            .request_json::<Value>(
                None,
                Method::POST,
                "/auth/passkey/enrollment/options",
                Some(json!({ "username": target.username, "code": code })),
            )
            .await?;
        let options = options.into_success()?;
        let display_name =
            sqlx::query_scalar::<_, String>("SELECT display_name FROM kival.users WHERE id = $1")
                .bind(target.id)
                .fetch_one(&kival.pool)
                .await?;
        assert_eq!(options["publicKey"]["user"]["name"], target.username);
        assert_eq!(options["publicKey"]["user"]["displayName"], display_name);
        assert!(options["ceremonyId"].is_string());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn initial_enrollment_code_requires_zero_active_passkeys(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let target = kival.create_user("passkey-enrollment-stale-code").await?;
        let code = issue_operator_enrollment_code(&kival, target.id, "enrollment").await?;

        TestPasskey::new().install(&kival, target.id).await?;
        let stale = kival
            .request(
                None,
                Method::POST,
                "/auth/passkey/enrollment/options",
                Some(json!({ "username": target.username, "code": code })),
            )
            .await?;
        assert_eq!(stale.status(), StatusCode::CONFLICT);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn concurrent_registration_consumes_code_and_ceremony_once(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let kival = TestKival::new(pool).await?;
        let user = kival.create_user("pk-concurrent-register").await?;
        let code = issue_operator_enrollment_code(&kival, user.id, "enrollment").await?;
        let options = kival
            .request_json::<Value>(
                None,
                Method::POST,
                "/auth/passkey/enrollment/options",
                Some(json!({ "username": user.username, "code": code })),
            )
            .await?
            .into_success()?;
        assert_eq!(options["publicKey"]["authenticatorSelection"]["residentKey"], "required");
        assert_eq!(options["publicKey"]["authenticatorSelection"]["requireResidentKey"], true);
        assert_eq!(options["publicKey"]["authenticatorSelection"]["userVerification"], "required");

        let key = TestPasskey::new();
        let finish = json!({
            "username": user.username,
            "code": code,
            "ceremonyId": options["ceremonyId"],
            "label": "integration test passkey",
            "credential": key.registration(&options)
        });
        let first = kival.request(
            None,
            Method::POST,
            "/auth/passkey/enrollment/finish",
            Some(finish.clone()),
        );
        let second = kival.request(
            None,
            Method::POST,
            "/auth/passkey/enrollment/finish",
            Some(finish.clone()),
        );
        let (first, second) = tokio::join!(first, second);
        let statuses = [first?.status(), second?.status()];
        assert_eq!(statuses.iter().filter(|status| status.is_success()).count(), 1);
        assert_eq!(statuses.iter().filter(|status| status.is_client_error()).count(), 1);

        let replay = kival
            .request(None, Method::POST, "/auth/passkey/enrollment/finish", Some(finish))
            .await?;
        assert!(replay.status().is_client_error());

        let active_passkeys = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM kival.passkey_credentials WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(active_passkeys, 1);
        let consumed_ceremonies = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM kival.webauthn_ceremonies
            WHERE user_id = $1
                AND kind = 'enrollment_registration'
                AND consumed_at IS NOT NULL
            "#,
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(consumed_ceremonies, 1);
        start_login(&kival, &user.username).await?;
        let retained_consumed_ceremonies = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
            FROM kival.webauthn_ceremonies
            WHERE user_id = $1
                AND kind = 'enrollment_registration'
                AND consumed_at IS NOT NULL
            "#,
        )
        .bind(user.id)
        .fetch_one(&kival.pool)
        .await?;
        assert_eq!(retained_consumed_ceremonies, 0);

        Ok(())
    }
}
