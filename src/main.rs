// mtrやtracerouteの出力は数十KB程度。誤送信で巨大な本文をパースしてメモリを使い切らないよう制限する
const MAX_BODY_BYTES: usize = 256 * 1024;

struct App {
    s3: aws_sdk_s3::Client,
    bucket: String,
    tokens: upstream_save::auth::Tokens,
    viewers: upstream_save::auth::Tokens,
    ripestat: upstream_save::enrich::Client,
}

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    lambda_http::tracing::init_default_subscriber();
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let app = App {
        s3: aws_sdk_s3::Client::new(&config),
        bucket: std::env::var("BUCKET")?,
        tokens: upstream_save::auth::Tokens::from_json(&std::env::var("TOKEN_HASHES")?)?,
        viewers: upstream_save::auth::Tokens::from_json(&std::env::var("VIEWER_TOKEN_HASHES")?)?,
        ripestat: upstream_save::enrich::Client::new(),
    };
    let app = &app;
    lambda_http::run(lambda_http::service_fn(move |req| handle(app, req))).await
}

fn json_response(
    status: u16,
    body: serde_json::Value,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    Ok(lambda_http::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(lambda_http::Body::from(body.to_string()))?)
}

const VIEWER_HTML: &str = include_str!("../viewer/index.html");
const VIEWER_JS: &str = include_str!("../viewer/viewer.js");
// CSSはindex.htmlに埋め込む。Lambdaの同時実行数を絞っているので、ページの読み込みで並行するリクエストを減らす
const VIEWER_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; img-src data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn static_response(
    content_type: &str,
    body: &'static str,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    Ok(lambda_http::Response::builder()
        .status(200)
        .header("content-type", content_type)
        .header("cache-control", "no-cache")
        .header("content-security-policy", VIEWER_CSP)
        .header("x-content-type-options", "nosniff")
        .body(lambda_http::Body::from(body))?)
}

async fn handle(
    app: &App,
    req: lambda_http::Request,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    use lambda_http::http::Method;

    // trace.shやiPhoneのショートカットは`/?...`に送るので、POSTはパスを見ない
    if req.method() == Method::POST {
        return ingest(app, req).await;
    }
    let path = req.uri().path().to_owned();
    let token = match req.headers().get("x-token") {
        None => None,
        Some(v) => Some(v.to_str().unwrap_or("")),
    };
    let access = upstream_save::view::access(&app.viewers, token);

    match (req.method(), path.as_str()) {
        (&Method::GET, "/") => return static_response("text/html; charset=utf-8", VIEWER_HTML),
        (&Method::GET, "/viewer.js") => {
            return static_response("text/javascript; charset=utf-8", VIEWER_JS);
        }
        (_, "/" | "/viewer.js") => {
            return json_response(405, serde_json::json!({"error": "method not allowed"}));
        }
        (&Method::GET, "/api/traces") => {
            let Some(access) = access else {
                return unauthorized();
            };
            return list_traces(app, access).await;
        }
        _ => {}
    }

    let Some(rest) = path.strip_prefix("/api/traces/") else {
        return not_found();
    };
    if let Some(r) = rest.strip_suffix("/public") {
        let Some(r) = upstream_save::view::TraceRef::parse(r) else {
            return not_found();
        };
        let public = match *req.method() {
            Method::PUT => true,
            Method::DELETE => false,
            _ => return json_response(405, serde_json::json!({"error": "use PUT or DELETE"})),
        };
        if access != Some(upstream_save::view::Access::Viewer) {
            return unauthorized();
        }
        return set_public(app, &r, public).await;
    }
    let Some(r) = upstream_save::view::TraceRef::parse(rest) else {
        return not_found();
    };
    if req.method() != Method::GET {
        return json_response(405, serde_json::json!({"error": "use GET"}));
    }
    let Some(access) = access else {
        return unauthorized();
    };
    get_trace(app, &r, access).await
}

fn unauthorized() -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    json_response(401, serde_json::json!({"error": "unauthorized"}))
}

fn not_found() -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    json_response(404, serde_json::json!({"error": "not found"}))
}

async fn list_traces(
    app: &App,
    access: upstream_save::view::Access,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    use futures::StreamExt as _;

    let public: std::collections::HashSet<_> = list_keys(app, "public/")
        .await?
        .iter()
        .filter_map(|k| upstream_save::view::TraceRef::from_public_key(k))
        .collect();
    let refs: Vec<_> = match access {
        upstream_save::view::Access::Viewer => list_keys(app, "traces/")
            .await?
            .iter()
            .filter_map(|k| upstream_save::view::TraceRef::from_trace_key(k))
            .collect(),
        upstream_save::view::Access::Anonymous => public.iter().cloned().collect(),
    };

    let mut stream = futures::stream::iter(refs)
        .map(|r| async move {
            let body = get_object(app, &r.trace_key()).await;
            (r, body)
        })
        .buffer_unordered(MAX_CONCURRENT_GETS);
    let mut summaries = Vec::new();
    while let Some((r, body)) = stream.next().await {
        let Some(body) = body? else {
            continue;
        };
        match serde_json::from_slice::<upstream_save::model::Trace>(&body) {
            Ok(trace) => {
                let is_public = public.contains(&r);
                summaries.push(upstream_save::view::Summary::new(r, trace, is_public));
            }
            Err(e) => tracing::warn!(error = %e, key = %r, "skipping unreadable trace"),
        }
    }
    summaries.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.key.cmp(&a.key)));
    json_response(200, serde_json::to_value(summaries)?)
}

const MAX_CONCURRENT_GETS: usize = 16;

async fn get_trace(
    app: &App,
    r: &upstream_save::view::TraceRef,
    access: upstream_save::view::Access,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    let public = exists(app, &r.public_key()).await?;
    if access == upstream_save::view::Access::Anonymous && !public {
        return not_found();
    }
    let Some(body) = get_object(app, &r.trace_key()).await? else {
        return not_found();
    };
    let trace: upstream_save::model::Trace = serde_json::from_slice(&body)?;
    let mut value = serde_json::to_value(trace)?;
    value["key"] = serde_json::json!(r);
    value["public"] = serde_json::json!(public);
    json_response(200, value)
}

async fn set_public(
    app: &App,
    r: &upstream_save::view::TraceRef,
    public: bool,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    if !exists(app, &r.trace_key()).await? {
        return not_found();
    }
    let key = r.public_key();
    if public {
        app.s3
            .put_object()
            .bucket(&app.bucket)
            .key(&key)
            .body(Vec::new().into())
            .send()
            .await?;
    } else {
        app.s3
            .delete_object()
            .bucket(&app.bucket)
            .key(&key)
            .send()
            .await?;
    }
    json_response(200, serde_json::json!({"key": r, "public": public}))
}

async fn list_keys(app: &App, prefix: &str) -> Result<Vec<String>, lambda_http::Error> {
    let mut pages = app
        .s3
        .list_objects_v2()
        .bucket(&app.bucket)
        .prefix(prefix)
        .into_paginator()
        .send();
    let mut keys = Vec::new();
    while let Some(page) = pages.next().await {
        keys.extend(
            page?
                .contents()
                .iter()
                .filter_map(|o| o.key().map(str::to_owned)),
        );
    }
    Ok(keys)
}

/// オブジェクトが無ければ`None`を返す
async fn get_object(app: &App, key: &str) -> Result<Option<Vec<u8>>, lambda_http::Error> {
    let out = match app
        .s3
        .get_object()
        .bucket(&app.bucket)
        .key(key)
        .send()
        .await
    {
        Ok(out) => out,
        Err(e) if e.as_service_error().is_some_and(|e| e.is_no_such_key()) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(Some(out.body.collect().await?.into_bytes().to_vec()))
}

async fn exists(app: &App, key: &str) -> Result<bool, lambda_http::Error> {
    match app
        .s3
        .head_object()
        .bucket(&app.bucket)
        .key(key)
        .send()
        .await
    {
        Ok(_) => Ok(true),
        Err(e) if e.as_service_error().is_some_and(|e| e.is_not_found()) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

async fn ingest(
    app: &App,
    req: lambda_http::Request,
) -> Result<lambda_http::Response<lambda_http::Body>, lambda_http::Error> {
    use lambda_http::RequestExt as _;

    let token = req
        .headers()
        .get("x-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some(client) = app.tokens.authenticate(token) else {
        return json_response(401, serde_json::json!({"error": "unauthorized"}));
    };

    let query = req.query_string_parameters();
    let label = query.first("label").map(str::to_owned);
    let declared = match query.first("format") {
        None => None,
        Some(s) => match upstream_save::model::Format::from_param(s) {
            Some(f) => Some(f),
            None => {
                return json_response(
                    400,
                    serde_json::json!({"error": format!("unknown format: {s}")}),
                );
            }
        },
    };
    let declared_af = match query.first("af") {
        None => None,
        Some("4") => Some(4),
        Some("6") => Some(6),
        Some(s) => {
            return json_response(
                400,
                serde_json::json!({"error": format!("af must be 4 or 6: {s}")}),
            );
        }
    };
    let source_ip: Option<std::net::IpAddr> = match req.request_context_ref() {
        Some(lambda_http::request::RequestContext::ApiGatewayV2(ctx)) => {
            ctx.http.source_ip.as_deref().and_then(|s| s.parse().ok())
        }
        _ => None,
    };

    let raw: &[u8] = req.body().as_ref();
    if raw.len() > MAX_BODY_BYTES {
        return json_response(
            413,
            serde_json::json!({"error": format!("body must be at most {MAX_BODY_BYTES} bytes")}),
        );
    }
    let body = String::from_utf8_lossy(raw);

    let now = chrono::Utc::now();
    let parsed = upstream_save::parse::parse(&body, declared);
    let af = declared_af.or_else(|| parsed.as_ref().ok().and_then(|p| infer_af(&p.hops)));
    let id = object_id(now, client, af);
    let month = now.format("%Y-%m");
    let raw_key = format!("raw/{month}/{id}.txt");

    if !put_new(app, &raw_key, "text/plain", raw.to_vec()).await? {
        return json_response(
            409,
            serde_json::json!({"error": "a record with the same key already exists", "raw_key": raw_key}),
        );
    }

    let parsed = match parsed {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, %raw_key, "parse failed");
            return json_response(
                422,
                serde_json::json!({"error": e.to_string(), "raw_key": raw_key}),
            );
        }
    };

    let mut hops = parsed.hops;
    let lookups = app
        .ripestat
        .lookup_all(hops.iter().filter_map(|h| h.ip).chain(source_ip))
        .await;
    upstream_save::enrich::apply(&mut hops, &lookups.infos);

    let trace = upstream_save::model::Trace {
        ts: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        client: client.to_owned(),
        label,
        target: query.first("target").map(str::to_owned).or(parsed.target),
        af,
        source_ip,
        source: source_ip.and_then(|ip| lookups.infos.get(&ip).cloned()),
        as_path: upstream_save::aspath::as_path(&hops),
        lookup_failed: lookups.failed,
        format: parsed.format,
        hops,
    };
    let key = format!("traces/{month}/{id}.json");
    if !put_new(
        app,
        &key,
        "application/json",
        serde_json::to_vec_pretty(&trace)?,
    )
    .await
    .inspect_err(|e| tracing::error!(error = ?e, %raw_key, "failed to store trace"))?
    {
        return json_response(
            409,
            serde_json::json!({"error": "a record with the same key already exists", "key": key, "raw_key": raw_key}),
        );
    }

    json_response(
        200,
        serde_json::json!({
            "key": key,
            "raw_key": raw_key,
            "format": trace.format,
            "hop_count": upstream_save::view::hop_count(&trace.hops),
            "as_path": trace.as_path,
            "source": trace.source,
            "lookup_failed": trace.lookup_failed,
        }),
    )
}

/// 既存のオブジェクトを上書きしない。同じキーが既にあれば`false`を返す
async fn put_new(
    app: &App,
    key: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<bool, lambda_http::Error> {
    let result = app
        .s3
        .put_object()
        .bucket(&app.bucket)
        .key(key)
        .content_type(content_type)
        .if_none_match("*")
        .body(body.into())
        .send()
        .await;
    match result {
        Ok(_) => Ok(true),
        // 412は既に存在する時、409は同じキーへの書き込みが競合した時
        Err(e)
            if matches!(
                e.raw_response().map(|r| r.status().as_u16()),
                Some(412 | 409)
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e.into()),
    }
}

fn infer_af(hops: &[upstream_save::model::Hop]) -> Option<u8> {
    hops.iter()
        .find_map(|h| h.ip)
        .map(|ip| if ip.is_ipv4() { 4 } else { 6 })
}

/// `20261004T100827Z-mbp-v6`という形式を返す。ファミリが分からない時は`-v<af>`を付けない
fn object_id(now: chrono::DateTime<chrono::Utc>, client: &str, af: Option<u8>) -> String {
    let ts = now.format("%Y%m%dT%H%M%SZ");
    match af {
        Some(af) => format!("{ts}-{client}-v{af}"),
        None => format!("{ts}-{client}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn object_id_format() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-04T10:08:27Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            crate::object_id(now, "mbp", Some(6)),
            "20261004T100827Z-mbp-v6"
        );
        assert_eq!(
            crate::object_id(now, "iphone", None),
            "20261004T100827Z-iphone"
        );
    }

    #[test]
    fn infer_af_from_first_responding_hop() {
        let hops = [
            upstream_save::model::Hop {
                hop: 1,
                ..Default::default()
            },
            upstream_save::model::Hop {
                hop: 2,
                ip: Some("2001:db8::1".parse().unwrap()),
                ..Default::default()
            },
        ];
        assert_eq!(crate::infer_af(&hops), Some(6));
        assert_eq!(crate::infer_af(&hops[..1]), None);
    }
}
