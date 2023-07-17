use std::sync::{atomic::AtomicUsize, Arc, Mutex};

use actix_web::{http::Version, web};
use asgi_spec::HTTPScope;
use mp::Process;
use pyo3::{
    exceptions::{PyException, PyValueError},
    pyfunction, pymodule,
    types::{PyBytes, PyDict, PyModule, PyTuple},
    wrap_pyfunction, IntoPy, Py, PyAny, PyResult, Python,
};

mod asgi_spec;
mod mp;

pub struct ServerInfo {
    pub host: String,
    pub port: u16,
}

#[pyfunction]
pub fn printone() -> PyResult<i8> {
    println!("1");
    Ok(1)
}

pub async fn handler(
    sender: web::Data<ipc_channel::ipc::IpcSender<HTTPScope>>,
    server: web::Data<ServerInfo>,
    request: actix_web::HttpRequest,
) -> actix_web::HttpResponse {
    let http_version = match request.version() {
        Version::HTTP_10 => "1.0".into(),
        Version::HTTP_11 => "1.1".into(),
        Version::HTTP_2 => "2.0".into(),
        _ => return actix_web::HttpResponse::VersionNotSupported().finish(),
    };
    let mut headers = Vec::new();
    for (header_name, header_val) in request.headers() {
        headers.push((
            header_name.as_str().as_bytes().to_vec(),
            header_val.as_bytes().to_vec(),
        ))
    }
    let asgi_scope = HTTPScope {
        headers,
        http_version,
        tp: "http".into(),
        spec_version: "2.3".into(),
        scheme: request.connection_info().scheme().to_string(),
        client: None,
        version: "2.3".into(),
        server: (server.host.clone(), server.port),
        method: request.method().to_string().to_uppercase(),
        path: request.path().to_string(),
        raw_path: request.path().as_bytes().to_vec(),
        query_string: request.query_string().to_string(),
        root_path: None,
    };

    sender.send(asgi_scope).unwrap();
    return actix_web::HttpResponse::Ok().body("{ \"meme\": \"A\"}");
}

async fn actix_main(
    host: String,
    port: u16,
    workers: usize,
    senders: Vec<ipc_channel::ipc::IpcSender<HTTPScope>>,
) -> anyhow::Result<()> {
    let workers_nums = Arc::new(AtomicUsize::new(0));
    let cloned_host = host.clone();
    let server = actix_web::HttpServer::new(move || {
        // let worker_num = workers_nums.fetch_add(1, std::sync::atomic::Ordering::Acquire);
        let num = workers_nums
            .clone()
            .fetch_add(1, std::sync::atomic::Ordering::Acquire);
        log::debug!(
            "Worker num assigned: {} with pid: {}",
            num,
            std::process::id()
        );
        let sender = senders[num].clone();
        actix_web::App::new()
            .app_data(web::Data::new(sender.clone()))
            .app_data(web::Data::new(ServerInfo {
                host: cloned_host.clone(),
                port,
            }))
            .wrap(actix_web::middleware::Logger::new(
                "\"%r\" \"-\" \"%s\" \"%a\" \"%D\"",
            ))
            .default_service(actix_web::web::route().to(handler))
    })
    .workers(workers)
    .bind((host, port))?;
    match server.run().await {
        Err(err) => return Err(err.into()),
        Ok(_) => {
            log::warn!("Exiting.");
        }
    };

    Ok(())
}

fn get_mppyfunc(
    python: Python<'_>,
    chchc: ipc_channel::ipc::IpcReceiver<HTTPScope>,
) -> PyResult<Py<PyAny>> {
    let chan_rc = Arc::new(Mutex::new(chchc));
    let ffff = move |args: &PyTuple, _kwargs: Option<&PyDict>| -> PyResult<()> {
        log::info!("Started process with pidpd: {}", std::process::id());
        log::info!("{} {:?}", args, _kwargs);
        let _: PyResult<()> = Python::with_gil(|py| {
            let module = py.import(args[0].to_string().as_str())?;
            let arc_chan = chan_rc.clone();
            let local_chan = arc_chan.lock().map_err(|err| {
                PyValueError::new_err(format!("Cannot aquire lock for ipc. Cause: {}", err))
            })?;
            let app = module.getattr(args[1].to_string().as_str())?.call0()?;
            log::info!("Imported");

            loop {
                match local_chan.recv() {
                    Err(err) => {
                        return Err(PyException::new_err(format!(
                            "Fuck channels. Cause: {}",
                            err
                        )))
                    }
                    Ok(msg) => {
                        log::info!("Recived under GIL, yolo.");
                        // let dickt = PyDict::new(py);
                        let pythonized_msg = pythonize::pythonize(py, &msg)?;
                        let pydickt = pythonized_msg.downcast::<PyDict>(py)?;
                        let mut pyheders = Vec::new();
                        for (name, val) in msg.headers {
                            pyheders.push((PyBytes::new(py, &name), PyBytes::new(py, &val)));
                        }
                        pydickt.set_item("headers", pyheders)?;
                        pydickt.set_item("raw_path", PyBytes::new(py, &msg.raw_path))?;
                        let res = app.call1((pydickt,))?;
                        log::info!("{}", res);
                    }
                }
            }
        });
        Ok(())
        // loop {
        //     match chchc.recv() {
        //         Err(err) => {
        //             log::info!("Cannot shit: {}.", err);
        //             return Ok(());
        //         }
        //         Ok(st) => {
        //             log::info!("Received in python {}", st);
        //         }
        //     };
        // }
    };
    Ok(pyo3::types::PyCFunction::new_closure(python, None, None, ffff)?.into_py(python))
}

#[pyfunction]
pub fn run(
    app_path: String,
    host: Option<String>,
    port: Option<u16>,
    workers: Option<usize>,
) -> PyResult<()> {
    let split: Vec<String> = app_path.split(":").map(String::from).collect();
    if split.len() != 2 {
        return Err(PyValueError::new_err(format!(
            "Cannot parse app path: {}",
            app_path
        )));
    }
    let workers_count = workers.unwrap_or(4);
    Python::with_gil(|py| {
        let mut mp = Process::new(py)?;
        let mut senders = Vec::new();
        for _ in 0..workers_count {
            let (s, r) = ipc_channel::ipc::channel()
                .expect("Cannot create IPC channels for python-rust communication.");
            mp.spawn(
                &get_mppyfunc(py, r)?,
                (split[0].clone(), split[1].clone()),
                None,
            )?;
            senders.push(s);
        }
        let res = py.allow_threads(move || {
            let runtime = match actix_rt::Runtime::new() {
                Err(err) => {
                    return Err(PyException::new_err(format!(
                        "Cannot start actix runtime: {}",
                        err
                    )));
                }
                Ok(val) => val,
            };
            match runtime.block_on(actix_main(
                host.unwrap_or("0.0.0.0".into()),
                port.unwrap_or(8000),
                workers.unwrap_or(4),
                senders.clone(),
            )) {
                Err(err) => {
                    return Err(PyException::new_err(format!(
                        "Cannot start server: {}",
                        err
                    )));
                }
                Ok(_) => Ok(()),
            }
        });
        mp.join()?;
        return res;
    })
}

#[pymodule]
#[pyo3(name = "_core")]
fn rasgi(_py: pyo3::Python<'_>, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();
    m.add_function(wrap_pyfunction!(printone, m)?)?;
    m.add_function(wrap_pyfunction!(run, m)?)?;
    Ok(())
}
