use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use super::driver::{AuthPolicy, Route};
use super::EdgeDriver;

const DEFAULT_ADMIN: &str = "127.0.0.1:2019";
const ROUTES_PATH: &str = "/config/apps/http/servers/olympus/routes";

pub struct CaddyDriver {
    admin: SocketAddr,
    hall_upstream: String,
    writer: Mutex<()>,
}

impl CaddyDriver {
    pub fn new(admin: SocketAddr, hall_upstream: impl Into<String>) -> Self {
        Self {
            admin,
            hall_upstream: hall_upstream.into(),
            writer: Mutex::new(()),
        }
    }

    pub fn localhost(hall_upstream: impl Into<String>) -> Self {
        Self::new(
            DEFAULT_ADMIN.parse().expect("valid Caddy admin address"),
            hall_upstream,
        )
    }

    pub fn render(&self, desired: &[Route]) -> Value {
        let mut routes = desired.to_vec();
        routes.sort_by(|left, right| left.id.cmp(&right.id));
        Value::Array(
            routes
                .iter()
                .map(|route| self.render_route(route))
                .collect(),
        )
    }

    fn render_route(&self, route: &Route) -> Value {
        // Remove caller-controlled identity before auth. Keep the Hall session
        // cookie until the forward-auth subrequest has cloned the request.
        let mut handlers = vec![json!({
            "handler": "headers",
            "request": { "delete": ["X-Olympus-*", "X-Auth-Request-*", "Remote-*"] }
        })];
        if route.auth_policy != AuthPolicy::Public {
            handlers.push(json!({
                "handler": "reverse_proxy",
                "upstreams": [{ "dial": self.hall_upstream }],
                "rewrite": { "method": "GET", "uri": "/api/edge/auth" },
                "headers": { "request": { "set": {
                    "X-Olympus-Route-Id": [route.id],
                    "X-Forwarded-Method": ["{http.request.method}"],
                    "X-Forwarded-Uri": ["{http.request.uri}"],
                    "X-Forwarded-Host": ["{http.request.host}"]
                }}},
                "handle_response": [{
                    "match": { "status_code": [2] },
                    "routes": [{ "handle": [{ "handler": "headers", "request": { "set": {
                        "X-Olympus-User": ["{http.reverse_proxy.header.X-Olympus-User}"],
                        "X-Olympus-Org": ["{http.reverse_proxy.header.X-Olympus-Org}"],
                        "X-Olympus-Session": ["{http.reverse_proxy.header.X-Olympus-Session}"]
                    }}}]}]
                }]
            }));
        }
        // Primary credentials are for Hall only and must never reach an app or
        // the file server, including on explicitly public routes.
        handlers.push(json!({
            "handler": "headers",
            "request": { "delete": ["Authorization", "Cookie"] }
        }));
        if let Some(upstream) = &route.upstream {
            handlers.push(json!({
                "handler": "reverse_proxy",
                "upstreams": [{ "dial": format!("{}:{}", upstream.host, upstream.port) }],
                "transport": { "protocol": "http", "versions": ["1.1", "2"] }
            }));
        } else if let Some(root) = &route.artifact_root {
            handlers.push(json!({
                "handler": "rewrite",
                "strip_path_prefix": route.path_prefix
            }));
            handlers.push(json!({ "handler": "file_server", "root": root, "hide": ["**/.*"] }));
        }
        json!({
            "@id": route.id,
            "match": [{ "path": [format!("{}*", route.path_prefix)] }],
            "handle": [{ "handler": "subroute", "routes": [{ "handle": handlers }] }],
            "terminal": true
        })
    }

    fn request(&self, method: &str, path: &str, body: Option<&[u8]>) -> Result<u16> {
        let mut stream = TcpStream::connect_timeout(&self.admin, Duration::from_secs(1))
            .with_context(|| format!("connecting to Caddy admin API at {}", self.admin))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        let body = body.unwrap_or_default();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", self.admin, self.admin, body.len())?;
        stream.write_all(body)?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        let status = response
            .split_whitespace()
            .nth(1)
            .context("malformed Caddy admin response")?
            .parse::<u16>()?;
        Ok(status)
    }
}

impl EdgeDriver for CaddyDriver {
    fn apply(&self, desired: &[Route]) -> Result<()> {
        let _writer = self.writer.lock().expect("Caddy writer mutex poisoned");
        let body = serde_json::to_vec(&self.render(desired))?;
        let status = self.request("PATCH", ROUTES_PATH, Some(&body))?;
        anyhow::ensure!(
            (200..300).contains(&status),
            "Caddy rejected desired routes with HTTP {status}"
        );
        Ok(())
    }

    fn healthy(&self) -> bool {
        self.request("GET", "/config/", None)
            .is_ok_and(|status| (200..300).contains(&status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn route_table_renders_deterministically_with_hardening() {
        let driver = CaddyDriver::localhost("127.0.0.1:8787");
        let rendered = driver.render(&[
            Route {
                id: "static-docs".into(),
                path_prefix: "/artifacts/docs/".into(),
                upstream: None,
                artifact_root: Some(PathBuf::from("/srv/artifacts/docs")),
                auth_policy: AuthPolicy::Public,
                websocket: false,
            },
            Route {
                id: "app-editor".into(),
                path_prefix: "/app/editor/".into(),
                upstream: Some(super::super::HostPort {
                    host: "127.0.0.1".into(),
                    port: 3100,
                }),
                artifact_root: None,
                auth_policy: AuthPolicy::SessionScoped,
                websocket: true,
            },
        ]);
        let snapshot = serde_json::to_string_pretty(&rendered).unwrap();
        assert!(snapshot.contains("X-Olympus-*"));
        assert!(snapshot.contains("/api/edge/auth"));
        assert!(snapshot.contains("strip_path_prefix"));
        assert!(snapshot.find("app-editor").unwrap() < snapshot.find("static-docs").unwrap());
    }

    #[test]
    fn protected_route_preserves_cookie_for_auth_then_strips_it_for_upstream() {
        let driver = CaddyDriver::localhost("127.0.0.1:8787");
        let rendered = driver.render(&[Route {
            id: "app-editor".into(),
            path_prefix: "/app/editor/".into(),
            upstream: Some(super::super::HostPort {
                host: "127.0.0.1".into(),
                port: 3100,
            }),
            artifact_root: None,
            auth_policy: AuthPolicy::SessionScoped,
            websocket: true,
        }]);
        let handlers = &rendered[0]["handle"][0]["routes"][0]["handle"];
        assert_eq!(handlers[0]["request"]["delete"][0], "X-Olympus-*");
        assert_eq!(handlers[1]["rewrite"]["uri"], "/api/edge/auth");
        assert_eq!(handlers[2]["request"]["delete"][1], "Cookie");
        assert_eq!(handlers[3]["handler"], "reverse_proxy");
    }
}
