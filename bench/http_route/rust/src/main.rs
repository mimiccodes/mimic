// http_route: a small JSON API (hyper on tokio; TOKIO_WORKER_THREADS sets
// the thread count). GET /users/{id} answers the user as JSON; an id that
// is not a number, or any other path, is a 404. bench/run.py measures it
// with load/main.rs.
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use tokio::net::TcpListener;

fn text(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    Response::builder().status(status).header("Content-Type", "text/plain; charset=utf-8").body(Full::new(Bytes::from_static(body.as_bytes()))).unwrap()
}

async fn route(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    if req.method() != Method::GET {
        return Ok(text(StatusCode::NOT_FOUND, "not found"));
    }
    if path == "/health" {
        return Ok(text(StatusCode::OK, "ok"));
    }
    match path.strip_prefix("/users/").and_then(|s| s.parse::<i64>().ok()) {
        Some(id) => {
            let body = format!(r#"{{"id":{id},"name":"user{id}","score":{},"active":{}}}"#, id * 7 % 100, id % 3 != 0);
            Ok(Response::builder().header("Content-Type", "application/json").body(Full::new(Bytes::from(body))).unwrap())
        }
        None => Ok(text(StatusCode::NOT_FOUND, "not found")),
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:18432").await.unwrap();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        stream.set_nodelay(true).unwrap();
        tokio::spawn(async move {
            let _ = http1::Builder::new().pipeline_flush(true).serve_connection(TokioIo::new(stream), service_fn(route)).await;
        });
    }
}
