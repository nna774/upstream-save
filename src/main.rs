struct App {
    s3: aws_sdk_s3::Client,
    bucket: String,
    tokens: upstream_save::auth::Tokens,
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
        .body(lambda_http::Body::from(body.to_string()))?)
}

async fn handle(
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
    let body = String::from_utf8_lossy(raw);

    let now = chrono::Utc::now();
    let parsed = upstream_save::parse::parse(&body, declared);
    let af = declared_af.or_else(|| parsed.as_ref().ok().and_then(|p| infer_af(&p.hops)));
    let id = object_id(now, client, af);
    let month = now.format("%Y-%m");
    let raw_key = format!("raw/{month}/{id}.txt");

    if !put_new(app, &raw_key, "text/plain; charset=utf-8", raw.to_vec()).await? {
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
    let infos = app
        .ripestat
        .lookup_all(hops.iter().filter_map(|h| h.ip).chain(source_ip))
        .await;
    upstream_save::enrich::apply(&mut hops, &infos);

    let trace = upstream_save::model::Trace {
        ts: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        client: client.to_owned(),
        label,
        target: query.first("target").map(str::to_owned).or(parsed.target),
        af,
        source_ip,
        source: source_ip.and_then(|ip| infos.get(&ip).cloned()),
        as_path: upstream_save::aspath::as_path(&hops),
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
    .await?
    {
        return json_response(
            409,
            serde_json::json!({"error": "a record with the same key already exists", "key": key}),
        );
    }

    json_response(
        200,
        serde_json::json!({
            "key": key,
            "raw_key": raw_key,
            "format": trace.format,
            "hop_count": trace.hops.len(),
            "as_path": trace.as_path,
            "source": trace.source,
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
