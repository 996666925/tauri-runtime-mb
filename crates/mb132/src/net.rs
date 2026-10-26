//! Network interception.
//!
//! Miniblink calls `mbOnLoadUrlBegin` on the blink thread before a request
//! leaves the process. A [`RequestHandler`] can rewrite the response, which is
//! how a Tauri runtime serves `tauri://localhost` assets and the IPC endpoint.

use std::ffi::c_void;

use mb132_sys as sys;

/// An intercepted request.
#[derive(Debug, Clone)]
pub struct Request {
    /// Fully qualified request url.
    pub url: String,
    /// Upper case HTTP method.
    pub method: String,
    /// Request headers as reported by the engine (may be empty).
    pub headers: Vec<(String, String)>,
    /// Raw POST body, empty for other methods.
    pub body: Vec<u8>,
    /// Engine job handle. Only needed when answering asynchronously.
    pub job: NetJob,
}

impl Request {
    /// Path component of the url, without the leading slash.
    pub fn path(&self) -> &str {
        let without_scheme = self.url.split_once("://").map(|x| x.1).unwrap_or(&self.url);
        without_scheme
            .split_once('/')
            .map(|x| x.1)
            .unwrap_or_default()
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
    }

    /// Authority (host[:port]) of the url.
    pub fn host(&self) -> &str {
        let without_scheme = self.url.split_once("://").map(|x| x.1).unwrap_or(&self.url);
        without_scheme.split(['/', '?', '#']).next().unwrap_or_default()
    }
}

/// A synthesized response.
#[derive(Debug, Clone, Default)]
pub struct Response {
    /// HTTP status code. Miniblink always reports 200; the value is advisory.
    pub status: u16,
    /// Response headers.
    pub headers: Vec<(String, String)>,
    /// Mime type; when absent, derived from the extension.
    pub mime: Option<String>,
    /// Response body.
    pub body: Vec<u8>,
}

impl Response {
    /// Builds a `200 OK` response.
    pub fn ok(body: impl Into<Vec<u8>>, mime: impl Into<String>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            mime: Some(mime.into()),
            body: body.into(),
        }
    }

    /// Builds a `404 Not Found` response.
    pub fn not_found() -> Self {
        Self {
            status: 404,
            headers: Vec::new(),
            mime: Some("text/plain".into()),
            body: b"Not Found".to_vec(),
        }
    }

    /// Adds a header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Outcome of an intercepted request.
#[derive(Debug, Clone)]
pub enum RequestAction {
    /// Let the engine perform the request over the network.
    Continue,
    /// Answer immediately with the given payload.
    Respond(Response),
    /// The handler already called [`NetJob::hold`] and will answer later with
    /// [`NetJob::respond_async`].
    Hold,
}

impl Default for RequestAction {
    fn default() -> Self {
        Self::Continue
    }
}

/// Decides what happens with an intercepted request.
pub trait RequestHandler: Send + Sync + 'static {
    /// Called on the blink thread, before the request leaves the process.
    fn on_request(&self, request: Request) -> RequestAction;
}

impl<F> RequestHandler for F
where
    F: Fn(Request) -> RequestAction + Send + Sync + 'static,
{
    fn on_request(&self, request: Request) -> RequestAction {
        (self)(request)
    }
}

/// Handle to an in-flight engine network job.
#[derive(Debug, Clone, Copy)]
pub struct NetJob {
    raw: sys::mbNetJob,
}

// SAFETY: the handle is an opaque engine pointer that is only dereferenced by
// the engine itself. All calls are internally synchronized by Miniblink.
unsafe impl Send for NetJob {}
unsafe impl Sync for NetJob {}

impl NetJob {
    pub(crate) fn from_raw(raw: sys::mbNetJob) -> Self {
        Self { raw }
    }

    /// Raw engine pointer.
    pub fn as_raw(self) -> sys::mbNetJob {
        self.raw
    }

    /// Whether the handle is null.
    pub fn is_null(self) -> bool {
        self.raw.is_null()
    }

    /// Keeps the job open so the response can be produced later.
    pub fn hold(self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbNetHoldJobToAsynCommit)(self.raw) };
        }
    }

    /// Resumes a job previously put on hold.
    pub fn resume(self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbNetContinueJob)(self.raw) };
        }
    }

    /// Cancels the request.
    pub fn cancel(self) {
        if let Some(api) = sys::api() {
            unsafe { (api.mbNetCancelRequest)(self.raw) };
        }
    }

    /// Rewrites the request url.
    pub fn redirect(self, url: &str) {
        if let Some(api) = sys::api()
            && let Ok(url) = crate::cstr(url)
        {
            unsafe { (api.mbNetChangeRequestUrl)(self.raw, url.as_ptr()) };
        }
    }

    /// Writes a response synchronously. Must be called on the blink thread,
    /// i.e. from inside [`RequestHandler::on_request`].
    pub fn respond(self, response: &Response) {
        self.write(response);
    }

    /// Writes the response from any thread, then resumes the job.
    ///
    /// Used for handlers that produce their payload asynchronously.
    pub fn respond_async(self, response: Response) {
        if self.raw.is_null() {
            return;
        }
        if std::env::var_os("MB_TRACE").is_some() {
            eprintln!(
                "[mb] respond_async job={:?} len={}",
                self.raw,
                response.body.len()
            );
        }
        let payload = Box::into_raw(Box::new((self.raw, response)));
        if let Some(api) = sys::api() {
            unsafe {
                (api.mbCallBlinkThreadAsync)(
                    Some(blink_respond_trampoline),
                    payload as *mut c_void,
                    std::ptr::null_mut(),
                )
            };
        }
    }

    fn write(self, response: &Response) {
        let Some(api) = sys::api() else { return };

        unsafe {
            if let Some(mime) = &response.mime
                && let Ok(mime) = crate::cstr(mime)
            {
                (api.mbNetSetMIMEType)(self.raw, mime.as_ptr());
            }

            for (name, value) in &response.headers {
                if let (Ok(name), Ok(value)) = (crate::cstr(name), crate::cstr(value)) {
                    (api.mbNetSetHTTPHeaderFieldUtf8)(
                        self.raw,
                        name.as_ptr(),
                        value.as_ptr(),
                        1, // response header
                    );
                }
            }

            if !response.body.is_empty() {
                (api.mbNetSetData)(
                    self.raw,
                    response.body.as_ptr() as *mut c_void,
                    response.body.len() as i32,
                );
            }
        }
    }
}

unsafe extern "system" fn blink_respond_trampoline(param1: *mut c_void, _param2: *mut c_void) {
    if param1.is_null() {
        return;
    }
    // SAFETY: the payload was allocated with `Box::into_raw` in `respond_async`.
    let payload = unsafe { Box::from_raw(param1 as *mut (sys::mbNetJob, Response)) };
    let (raw, response) = *payload;
    let job = NetJob::from_raw(raw);
    if std::env::var_os("MB_TRACE").is_some() {
        eprintln!("[mb] blink respond job={raw:?} len={}", response.body.len());
    }
    job.write(&response);
    job.resume();
}

/// Reads the POST body out of an engine job.
/// Raw request headers as `(name, value)` pairs.
///
/// Must run on the blink thread, which is where `mbOnLoadUrlBegin` fires.
pub(crate) fn read_request_headers(job: sys::mbNetJob) -> Vec<(String, String)> {
    let Some(api) = sys::api() else {
        return Vec::new();
    };

    let mut headers = Vec::new();
    // SAFETY: called from the blink thread with a live job.
    let mut node = unsafe { (api.mbNetGetRawHttpHeadInBlinkThread)(job) };
    while !node.is_null() {
        // SAFETY: the engine owns the list for the duration of the callback.
        let data = unsafe { (*node).data };
        if !data.is_null() {
            let line = unsafe { crate::from_cstr(data) };
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim();
                // Skip the request line (`POST /path HTTP/1.1`).
                if !name.is_empty() && !name.contains(' ') && !name.contains('/') {
                    headers.push((name.to_string(), value.trim().to_string()));
                }
            }
        }
        node = unsafe { (*node).next };
    }
    headers
}

pub(crate) fn read_post_body(job: sys::mbNetJob) -> Vec<u8> {
    let Some(api) = sys::api() else {
        return Vec::new();
    };
    if job.is_null() {
        return Vec::new();
    }

    let mut body = Vec::new();
    unsafe {
        let elements = (api.mbNetGetPostBody)(job);
        if elements.is_null() {
            return body;
        }

        let count = (*elements).element_size;
        let list = (*elements).element;
        if !list.is_null() {
            for index in 0..count {
                let element = *list.add(index);
                if element.is_null() {
                    continue;
                }
                // 0 == mbHttBodyElementTypeData
                if (*element).element_type != 0 {
                    continue;
                }
                let data = (*element).data;
                if data.is_null() || (*data).data.is_null() {
                    continue;
                }
                let slice =
                    std::slice::from_raw_parts((*data).data as *const u8, (*data).length);
                body.extend_from_slice(slice);
            }
        }

        (api.mbNetFreePostBodyElements)(elements);
    }
    body
}
