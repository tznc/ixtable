//! The live client over HTTP against a mock ixtable Cloud: the session lifecycle in the
//! sealed store, Edge Function errors, and PostgREST reads.

use super::{
    client::LiveServices,
    mockhttp::{Request, Response, Server, lock_env},
    services::CloudServices,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, MutexGuard};

type Log = Arc<Mutex<Vec<(String, String, Value, std::collections::HashMap<String, String>)>>>;

struct Cloud {
    _env: MutexGuard<'static, ()>,
    server: Option<Server>,
    log: Log,
}

fn session_reply(access: &str, refresh: &str, expires_in: i64) -> Value {
    json!({
        "access_token": access,
        "refresh_token": refresh,
        "expires_in": expires_in,
        "user": { "id": "u-1", "email": "ada@example.test" },
    })
}

/// Points the client at a fresh mock whose answers `route` decides. Each mock has its own
/// URL, so each test has its own stored session.
fn cloud(route: impl Fn(&str, &str, &Value) -> (u16, Value) + Send + Sync + 'static) -> Cloud {
    let env = lock_env();
    let log: Log = Default::default();
    let seen = log.clone();
    let server = Server::start(move |request: Request| {
        let body = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        seen.lock().unwrap().push((request.method.clone(), request.path.clone(), body.clone(), request.headers));
        let (status, reply) = route(&request.method, &request.path, &body);
        Response::json(status, &reply)
    });
    std::env::set_var("IXTABLE_CLOUD_URL", &server.url);
    std::env::set_var("IXTABLE_CLOUD_ANON_KEY", "test-anon");
    std::env::set_var("IXTABLE_CLOUD_SITE_URL", "https://site.example/");
    Cloud { _env: env, server: Some(server), log }
}

impl Cloud {
    fn paths(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().map(|(m, p, _, _)| format!("{m} {p}")).collect()
    }
}

fn stored() -> Option<String> {
    crate::cloud::commands::cloud_auth_storage_get("main".into(), "sb-ixtable-auth".into()).unwrap()
}

#[test]
fn signs_in_keeps_the_session_in_the_sealed_store_and_signs_out_everywhere() {
    let cloud = cloud(|_, path, _| match path {
        p if p.starts_with("/auth/v1/token?grant_type=password") => (200, session_reply("access-1", "refresh-1", 3600)),
        p if p.starts_with("/auth/v1/logout") => (204, Value::Null),
        _ => (404, json!({})),
    });
    let session = LiveServices.sign_in("ada@example.test", "correct-horse").unwrap().unwrap();
    assert_eq!(
        (session.access_token.as_str(), session.user_id.as_str(), session.email.as_deref()),
        ("access-1", "u-1", Some("ada@example.test"))
    );
    assert!(stored().unwrap().contains("refresh-1"));
    // A fresh session loads without another request.
    let before = cloud.paths().len();
    assert_eq!(LiveServices.load_session().unwrap().access_token, "access-1");
    assert_eq!(cloud.paths().len(), before);
    let (_, _, body, headers) = cloud.log.lock().unwrap()[0].clone();
    assert_eq!(body, json!({ "email": "ada@example.test", "password": "correct-horse" }));
    assert_eq!(headers.get("apikey").map(String::as_str), Some("test-anon"));

    LiveServices.sign_out();
    assert!(stored().is_none());
    assert!(LiveServices.load_session().is_none());
    assert!(cloud.paths().iter().any(|p| p.starts_with("POST /auth/v1/logout?scope=local")));
}

#[test]
fn a_wrong_password_is_explained_without_storing_anything() {
    let _cloud = cloud(|_, _, _| (400, json!({ "error_description": "Invalid login credentials" })));
    let error = LiveServices.sign_in("ada@example.test", "nope-nope-nope").unwrap_err();
    assert_eq!(error.code, "UNAUTHENTICATED");
    assert!(stored().is_none());
}

#[test]
fn an_expiring_session_is_refreshed_and_a_refused_refresh_ends_it() {
    let refused = Arc::new(Mutex::new(false));
    let flag = refused.clone();
    let cloud = cloud(move |_, path, body| {
        if path.starts_with("/auth/v1/token?grant_type=password") {
            // Expires within the refresh margin.
            return (200, session_reply("access-1", "refresh-1", 10));
        }
        if path.starts_with("/auth/v1/token?grant_type=refresh_token") {
            assert!(body["refresh_token"].as_str().unwrap().starts_with("refresh-"));
            return if *flag.lock().unwrap() {
                (400, json!({ "error_description": "Invalid Refresh Token" }))
            } else {
                (200, session_reply("access-2", "refresh-2", 3600))
            };
        }
        (404, json!({}))
    });
    LiveServices.sign_in("ada@example.test", "correct-horse").unwrap();
    assert_eq!(LiveServices.load_session().unwrap().access_token, "access-2");
    assert!(stored().unwrap().contains("refresh-2"));
    assert!(cloud.paths().iter().any(|p| p.contains("grant_type=refresh_token")));

    // Force the stored session to expire, then have the server refuse the refresh token.
    let mut session: Value = serde_json::from_str(&stored().unwrap()).unwrap();
    session["expires_at"] = json!(1);
    crate::cloud::commands::cloud_auth_storage_set("main".into(), "sb-ixtable-auth".into(), session.to_string())
        .unwrap();
    *refused.lock().unwrap() = true;
    assert!(LiveServices.load_session().is_none());
    assert!(stored().is_none(), "a refused refresh token ends the session");
}

#[test]
fn password_reset_asks_for_the_site_reset_page() {
    let cloud = cloud(|_, _, _| (200, json!({})));
    LiveServices.request_password_reset("ada@example.test").unwrap();
    let paths = cloud.paths();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].starts_with("POST /auth/v1/recover?redirect_to=https%3A%2F%2Fsite.example%2Freset-password"), "{paths:?}");
}

#[test]
fn edge_function_errors_keep_their_code_status_and_details() {
    let cloud = cloud(|_, path, body| match path {
        p if p.starts_with("/auth/v1/token") => (200, session_reply("jwt-1", "refresh-1", 3600)),
        "/functions/v1/publish-checkpoint" => (
            409,
            json!({ "error": { "code": "VERSION_CONFLICT", "message": "Head moved", "details": { "headVersionId": "v9" } } }),
        ),
        "/functions/v1/echo" => (200, body.clone()),
        _ => (404, json!({})),
    });
    LiveServices.sign_in("ada@example.test", "correct-horse").unwrap();
    let echoed = LiveServices.invoke("echo", json!({ "a": 1 })).unwrap();
    assert_eq!(echoed, json!({ "a": 1 }));
    let error = LiveServices.invoke("publish-checkpoint", json!({})).unwrap_err();
    assert_eq!((error.code.as_str(), error.status, error.detail("headVersionId")), ("VERSION_CONFLICT", Some(409), Some("v9")));
    let log = cloud.log.lock().unwrap();
    let (_, _, _, headers) = log.iter().find(|(_, p, _, _)| p == "/functions/v1/echo").unwrap();
    assert_eq!(headers.get("authorization").map(String::as_str), Some("Bearer jwt-1"));
    assert_eq!(headers.get("apikey").map(String::as_str), Some("test-anon"));
}

#[test]
fn functions_need_a_session() {
    let _cloud = cloud(|_, _, _| (200, json!({})));
    assert_eq!(LiveServices.invoke("echo", json!({})).unwrap_err().code, "UNAUTHENTICATED");
}

#[test]
fn reads_apps_and_member_apps_through_postgrest_filters() {
    let cloud = cloud(|_, path, _| {
        if path.starts_with("/auth/v1/token") {
            return (200, session_reply("jwt-1", "refresh-1", 3600));
        }
        if path.starts_with("/rest/v1/cloud_apps") {
            return (
                200,
                json!([{
                    "id": "app-1", "org_id": "org-1", "owner_id": "u-1", "name": "CRM",
                    "document_id": "doc", "backups_enabled": true, "head_version_id": "v1",
                }]),
            );
        }
        if path.starts_with("/rest/v1/app_members") {
            return (
                200,
                json!([
                    { "app_id": "app-1", "role_id": "r1", "status": "active", "cloud_apps": { "id": "app-1", "name": "CRM" } },
                    { "app_id": "app-2", "role_id": "r9", "status": "active", "cloud_apps": [{ "id": "app-2", "name": "Orders" }] },
                ]),
            );
        }
        if path.starts_with("/rest/v1/app_roles") {
            return (200, json!([{ "app_id": "app-1", "id": "r1", "name": "Sales rep" }]));
        }
        (404, json!({}))
    });
    LiveServices.sign_in("ada@example.test", "correct-horse").unwrap();
    let app = LiveServices.get_app("app-1").unwrap().unwrap();
    assert_eq!(
        (app.id.as_str(), app.owner_id.as_str(), app.backups_enabled, app.head_version_id.as_deref()),
        ("app-1", "u-1", true, Some("v1"))
    );
    let apps = LiveServices.list_member_apps().unwrap();
    assert_eq!(
        apps.iter().map(|a| (a.app_id.as_str(), a.name.as_str(), a.role_name.as_str())).collect::<Vec<_>>(),
        [("app-1", "CRM", "Sales rep"), ("app-2", "Orders", "")]
    );
    let paths = cloud.paths();
    assert!(paths.iter().any(|p| p.contains("/rest/v1/cloud_apps?") && p.contains("id=eq.app-1")), "{paths:?}");
    assert!(paths.iter().any(|p| p.contains("/rest/v1/app_members?") && p.contains("user_id=eq.u-1") && p.contains("status=eq.active")), "{paths:?}");
}

#[test]
fn an_unreachable_cloud_reads_as_offline_and_keeps_the_stored_session() {
    let mut cloud = cloud(|_, path, _| {
        if path.starts_with("/auth/v1/token?grant_type=password") {
            return (200, session_reply("jwt-1", "refresh-1", 3600));
        }
        (200, json!([]))
    });
    LiveServices.sign_in("ada@example.test", "correct-horse").unwrap();
    drop(cloud.server.take());
    let error = LiveServices.list_organizations().unwrap_err();
    assert_eq!(error.code, "CLOUD_OFFLINE");
    assert!(stored().is_some());
}
