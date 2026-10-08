# Application execution

Flint connects applications that execute code to a backend that coordinates requests and keeps workflow records.

## Language

**Host application**:
A running application process whose own language runtime executes code submitted through Flint, such as Maya, 3ds Max, or a Python process.
_Avoid_: Host for a network address; use endpoint address.

**Host kind**:
A built-in host integration recognized by Flint, represented by `HostKind` in the shared contracts. An application integration owns its runtime selection and execution behavior; a standalone integration identifies a fixed source language. Recognizing a kind does not imply that Flint can discover or attach it. Bridge registration identifiers remain open strings, so independently loaded Bridges can identify hosts outside this built-in set.
_Avoid_: Language or runtime kind for this integration identity.

**Host runtime**:
The interpreter or managed runtime inside a host application that executes submitted code. Its version is reported by a connected instance.
_Avoid_: Runtime without qualification.

**Host candidate**:
A discovered host application process that may support a Bridge but has not necessarily connected to the backend.
_Avoid_: Instance for a merely discovered process.

**Window preview**:
A reduced image of a host candidate's own main window, captured on request. It never contains other windows or screen content, and an unavailable preview has a stated reason.
_Avoid_: Screenshot, thumbnail.

**Bridge**:
The host-side connector that registers an application with the backend and carries execution requests and results. It runs within the host process.
_Avoid_: Host application, instance.

**Bridge core**:
The shared native component of a Bridge that owns its connection, registration, heartbeat, framing, and reconnection behavior. It orchestrates each execution's preparation, invocation, cancellation, and result through the host's execution capabilities; the host runtime executes the code. It holds a process-wide claim so a host process runs at most one Bridge.

**Execution capabilities**:
The host-specific part of execution that a host adapter supplies to the Bridge core: a scheduler that runs the core's callbacks on the thread where the host executes code, and an executor that prepares and runs submitted code in the host runtime. When and whether each step happens, and how its outcome is reported, belong to the core.

**Execution coordinator**:
The part of the Bridge core that coordinates scheduling, preparation, invocation, cancellation, and completion of an execution through its execution binding.
_Avoid_: Host for this core component.

**Execution binding**:
The host-supplied ABI structure containing the context and functions the Bridge core uses to schedule, prepare, run, and discard work, and to release the binding.
_Avoid_: Host, callbacks for the binding as a whole.

**Execution adapter**:
The platform component that connects an execution binding to the host's executor and scheduler, translating language values, asynchronous completions, and object lifetimes.

**Prepared result**:
The host-runtime value produced by preparing an execution request and supplied to its run step. It carries the code or resources the executor needs to run that request.

**Bridge manager**:
The platform component that creates, reuses, configures, and releases a host's Bridge using the host's execution capabilities. It manages Bridge resources; connection policy belongs to the Bridge core.

**Attach**:
Starting a Bridge from outside its host process by injecting the Bridge into the running process, rather than the host loading the Bridge itself. Both paths connect the same Bridge to the backend; attach lets flint connect a host without the user running flint's code inside it. Attaching to a process whose Bridge it can reach applies the new settings to that Bridge, and a failure is reported back to the requester.
_Avoid_: Inject for the whole operation; injection is only the entry step.

**Process claim**:
The host process's exclusive right to run one Bridge, held by its Bridge core through an operating-system lock keyed by the process. It prevents a second Bridge, whatever runtime or Bridge version attempts it, and releases when that Bridge is destroyed or the process exits.

**Connection obstacle**:
Why a Bridge is retrying its connection: the Bridge endpoint is unreachable, the backend did not complete registration, or a registered session was lost. It belongs to the Bridge's connection state and ends with that state, so a successful or reset connection carries none.
_Avoid_: Last error, for an obstacle or for any failure in general.

**Rejected operation**:
A request to a Bridge, or to start one, that was refused without changing any state, such as settings refused while host code executes or a Bridge that could not be created. Its reason returns to whoever made the request and never appears as a connection obstacle.

**Host adapter**:
The host-side code that supplies its application's execution capabilities, along with its startup, settings storage, and teardown.

**Backend**:
The single local service for the current user that accepts control commands and Bridge connections, coordinates executions, and owns workflow records. CLI sessions reuse this service independently of their working directory.
_Avoid_: Server or core when referring to this service as a whole.

**Connected instance**:
A transient backend registration of a Bridge in a host application, identified by an instance ID. Reconnection can produce a new instance ID for the same Bridge.
_Avoid_: Host application for the registered connection.

**Control client**:
The CLI-side component that sends a command to the backend and receives its response. It is separate from the host-side Bridge.
_Avoid_: Client without qualification.

**Control endpoint**:
The backend address and port used by control clients to send commands.

**Bridge endpoint**:
The backend address and port used by Bridges for registration, heartbeat, and execution traffic.
_Avoid_: Registry endpoint or host endpoint for this connection boundary.

**Workflow**:
A durable group of related executions with its own identifier, name, and description. Its records remain after a backend restart.

**Execution**:
One submitted code run associated with a workflow and a connected instance, together with its recorded status, output, and error information. A lost connection can leave the host outcome unknown.

**Wire protocol**:
The versioned messages and length-prefixed framing exchanged between the backend, control clients, and Bridges.
_Avoid_: Protobuf alone for the complete wire contract.
