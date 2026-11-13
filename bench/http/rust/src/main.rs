// http: a hello-world server (hyper on tokio; TOKIO_WORKER_THREADS sets the
// thread count); bench/run.py measures it with load/main.go
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use tokio::net::TcpListener;

async fn hello(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::builder()
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(b"Hello, World!")))
        .unwrap())
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:18432").await.unwrap();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        stream.set_nodelay(true).unwrap();
        tokio::spawn(async move {
            let _ = http1::Builder::new().pipeline_flush(true).serve_connection(TokioIo::new(stream), service_fn(hello)).await;
        });
    }
}
