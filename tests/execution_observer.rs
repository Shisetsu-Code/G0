#[path = "support/program_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use g0::execution::{
    Cancellation, ExecutionLimits, ExecutionObserver, Executor, RuntimeError, TraceEvent,
};
use g0::gir::NodeId;
use g0::value::Value;
use std::sync::{Arc, Condvar, Mutex, mpsc};

struct Gate {
    arrived: mpsc::Sender<(String, NodeId)>,
    released: Mutex<bool>,
    wake: Condvar,
    events: Mutex<Vec<TraceEvent>>,
}
impl ExecutionObserver for Gate {
    fn before_node(
        &self,
        graph: &str,
        node: NodeId,
        cancellation: &Cancellation,
    ) -> Result<(), RuntimeError> {
        self.arrived.send((graph.into(), node)).unwrap();
        let mut released = self.released.lock().unwrap();
        while !*released && !cancellation.is_cancelled() {
            released = self.wake.wait(released).unwrap();
        }
        Ok(())
    }
    fn after_node(&self, event: &TraceEvent) -> Result<(), RuntimeError> {
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

#[test]
fn pauses_before_actual_execution_and_continues_nested_graphs() {
    let program = fixture::call_program().validated_contract().unwrap();
    let (sender, receiver) = mpsc::channel();
    let gate = Arc::new(Gate {
        arrived: sender,
        released: Mutex::new(false),
        wake: Condvar::new(),
        events: Mutex::new(vec![]),
    });
    std::thread::scope(|scope| {
        let worker_gate = gate.clone();
        let worker = scope.spawn(|| {
            let mut executor = Executor::new(&program, ExecutionLimits::default()).unwrap();
            executor.set_observer(worker_gate);
            executor.run_entry()
        });
        let (graph, _) = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(graph, "main");
        assert!(gate.events.lock().unwrap().is_empty());
        *gate.released.lock().unwrap() = true;
        gate.wake.notify_all();
        assert_eq!(worker.join().unwrap().unwrap(), [Value::Integer(42)]);
    });
    let events = gate.events.lock().unwrap();
    assert!(events.iter().any(|event| event.graph == "worker"));
    assert_eq!(events.last().unwrap().outputs, [Value::Integer(42)]);
}

#[test]
fn cancelling_a_paused_node_prevents_its_execution() {
    let program = fixture::call_program().validated_contract().unwrap();
    let (sender, receiver) = mpsc::channel();
    let gate = Arc::new(Gate {
        arrived: sender,
        released: Mutex::new(false),
        wake: Condvar::new(),
        events: Mutex::new(vec![]),
    });
    let mut executor = Executor::new(&program, ExecutionLimits::default()).unwrap();
    let cancellation = executor.cancellation();
    executor.set_observer(gate.clone());
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || executor.run_entry());
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        cancellation.cancel();
        gate.wake.notify_all();
        assert_eq!(worker.join().unwrap(), Err(RuntimeError::Cancelled));
    });
    assert!(gate.events.lock().unwrap().is_empty());
}
