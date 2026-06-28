//! HTTP 中间件
//!
//! - Trace ID 中间件：为每个请求生成唯一追踪 ID
//! - 请求日志中间件：记录请求和响应信息

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    response::Response,
};
use std::time::Instant;
use tower::{Layer, Service};
use uuid::Uuid;

/// Trace ID 中间件
///
/// 为每个请求生成唯一的追踪 ID：
/// - 如果请求头中已有 X-Trace-Id，则使用它
/// - 否则生成新的 UUID v4
/// - 将 Trace ID 放入请求扩展和响应头中
#[derive(Debug, Clone)]
pub struct TraceIdLayer;

impl<S> Layer<S> for TraceIdLayer {
    type Service = TraceIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceIdService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct TraceIdService<S> {
    inner: S,
}

impl<S> Service<Request> for TraceIdService<S>
where
    S: Service<Request, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        // 从请求头获取或生成新的 Trace ID
        let trace_id = req
            .headers()
            .get("x-trace-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // 放入请求扩展
        let trace_id_clone = trace_id.clone();
        req.extensions_mut().insert(TraceId(trace_id_clone));

        // 调用内部服务
        let fut = self.inner.call(req);

        Box::pin(async move {
            let mut response = fut.await?;

            // 将 Trace ID 放入响应头
            if let Ok(header_value) = HeaderValue::from_str(&trace_id) {
                response
                    .headers_mut()
                    .insert(HeaderName::from_static("x-trace-id"), header_value);
            }

            Ok(response)
        })
    }
}

/// Trace ID 提取器（用于 Extension 提取）
#[derive(Debug, Clone)]
pub struct TraceId(pub String);

impl std::ops::Deref for TraceId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 请求日志中间件
///
/// 记录每个请求的：
/// - 请求方法、路径
/// - 状态码
/// - 响应时间
/// - Trace ID
#[derive(Debug, Clone)]
pub struct RequestLogLayer;

impl<S> Layer<S> for RequestLogLayer {
    type Service = RequestLogService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestLogService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLogService<S> {
    inner: S,
}

impl<S> Service<Request> for RequestLogService<S>
where
    S: Service<Request, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let start = Instant::now();

        // 获取 Trace ID
        let trace_id = req
            .extensions()
            .get::<TraceId>()
            .map(|t| t.0.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let fut = self.inner.call(req);

        Box::pin(async move {
            let response = fut.await?;
            let duration = start.elapsed();
            let status = response.status();

            // 记录请求日志
            tracing::info!(
                target: "http_request",
                trace_id = %trace_id,
                method = %method,
                path = %path,
                status = %status.as_u16(),
                duration_ms = %duration.as_millis(),
                "{} {} {} ({}ms)",
                method,
                path,
                status.as_u16(),
                duration.as_millis()
            );

            Ok(response)
        })
    }
}

/// 初始化日志系统
///
/// 同时输出到控制台和文件
/// - 控制台：彩色、简洁
/// - 文件：JSON 格式、详细信息
///
/// 返回的 guard 必须在整个程序生命周期内保持，
/// 否则日志写入可能会丢失
pub fn init_logging() -> impl Drop {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{fmt, EnvFilter, Registry};

    // 日志文件配置
    let file_appender = tracing_appender::rolling::daily("./logs", "subhuti.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    // 控制台格式化
    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_span_events(FmtSpan::NONE)
        .compact();

    // 文件格式化（JSON）
    let file_layer = fmt::layer()
        .json()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(file_writer);

    // 日志级别过滤
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=off,hyper=off,reqwest=off"));

    Registry::default()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    guard
}
