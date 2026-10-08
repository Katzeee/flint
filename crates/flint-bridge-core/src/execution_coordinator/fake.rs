//! A host whose scheduling and completions each test controls.
use super::{Step, Ticket};
use crate::execution_binding::ExecutionBinding;
use crate::ffi::{flint_step_fail, flint_step_output, flint_step_succeed, flint_ticket_run};
use serde_json::Value;
use std::{
    collections::VecDeque,
    ffi::{c_char, CStr},
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Complete,
    Hold,
    Fail,
}

pub(crate) const PREPARED: usize = 7;

pub(crate) struct Fake {
    pub(crate) prepare: Mutex<Mode>,
    pub(crate) run: Mutex<Mode>,
    pub(crate) refuse: AtomicBool,
    pub(crate) released: AtomicUsize,
    pub(crate) during_prepare: Mutex<Option<Box<dyn Fn() + Send>>>,
    calls: Mutex<Vec<String>>,
    requests: Mutex<Vec<Value>>,
    tickets: Mutex<VecDeque<usize>>,
    held: Mutex<Option<usize>>,
}

impl Fake {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            prepare: Mutex::new(Mode::Complete),
            run: Mutex::new(Mode::Complete),
            refuse: AtomicBool::new(false),
            released: AtomicUsize::new(0),
            during_prepare: Mutex::new(None),
            calls: Mutex::new(vec![]),
            requests: Mutex::new(vec![]),
            tickets: Mutex::new(VecDeque::new()),
            held: Mutex::new(None),
        })
    }

    pub(crate) fn execution_binding(self: &Arc<Self>) -> ExecutionBinding {
        ExecutionBinding {
            context: Arc::into_raw(self.clone()) as usize,
            post,
            prepare,
            run,
            discard,
            release,
        }
    }

    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn take_request(&self) -> Option<Value> {
        self.requests.lock().unwrap().pop()
    }

    pub(crate) fn take_ticket(&self, timeout: Duration) -> Option<*mut Ticket> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(ticket) = self.tickets.lock().unwrap().pop_front() {
                return Some(ticket as *mut Ticket);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Runs the next posted ticket on this thread, as a host loop would.
    pub(crate) fn run_ticket(&self, timeout: Duration) -> bool {
        let Some(ticket) = self.take_ticket(timeout) else {
            return false;
        };
        unsafe { flint_ticket_run(ticket) };
        true
    }

    pub(crate) fn take_held(&self) -> Option<*mut Step> {
        self.held
            .lock()
            .unwrap()
            .take()
            .map(|step| step as *mut Step)
    }

    pub(crate) fn held(&self) -> Option<*const Step> {
        self.held.lock().unwrap().map(|step| step as *const Step)
    }
}

pub(crate) unsafe fn write(step: *const Step, stdout: &str, stderr: &str) -> bool {
    flint_step_output(
        step,
        stdout.as_ptr(),
        stdout.len(),
        stderr.as_ptr(),
        stderr.len(),
    )
}

unsafe fn fake<'a>(context: usize) -> &'a Fake {
    &*(context as *const Fake)
}

unsafe extern "C" fn post(context: usize, ticket: *mut Ticket) -> bool {
    let fake = fake(context);
    if fake.refuse.load(Ordering::SeqCst) {
        return false;
    }
    fake.tickets.lock().unwrap().push_back(ticket as usize);
    true
}

unsafe extern "C" fn prepare(context: usize, request: *const c_char, step: *mut Step) {
    let fake = fake(context);
    fake.calls.lock().unwrap().push("prepare".into());
    fake.requests
        .lock()
        .unwrap()
        .push(serde_json::from_str(CStr::from_ptr(request).to_str().unwrap()).unwrap());
    if let Some(during) = fake.during_prepare.lock().unwrap().as_ref() {
        during();
    }
    let mode = *fake.prepare.lock().unwrap();
    match mode {
        Mode::Complete => {
            write(step, "prepared\n", "");
            flint_step_succeed(step, PREPARED);
        }
        Mode::Hold => *fake.held.lock().unwrap() = Some(step as usize),
        Mode::Fail => flint_step_fail(step, c"prepare trace".as_ptr(), ptr::null()),
    }
}

unsafe extern "C" fn run(context: usize, result_id: usize, step: *mut Step) {
    let fake = fake(context);
    fake.calls.lock().unwrap().push(format!("run {result_id}"));
    write(step, "ran\n", "");
    let mode = *fake.run.lock().unwrap();
    match mode {
        Mode::Complete => flint_step_succeed(step, 0),
        Mode::Hold => *fake.held.lock().unwrap() = Some(step as usize),
        Mode::Fail => flint_step_fail(step, ptr::null(), ptr::null()),
    }
}

unsafe extern "C" fn discard(context: usize, result_id: usize) {
    fake(context)
        .calls
        .lock()
        .unwrap()
        .push(format!("discard {result_id}"));
}

unsafe extern "C" fn release(context: usize) {
    let fake = Arc::from_raw(context as *const Fake);
    fake.released.fetch_add(1, Ordering::SeqCst);
}
