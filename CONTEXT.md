# Application execution

Flint connects applications that execute code to a backend that coordinates requests and keeps workflow records.

## Language

**Host application**:
A running application process whose own language runtime executes code submitted through Flint, such as Maya, 3ds Max, or a Python process.
_Avoid_: Host for a network address; use endpoint address.

**Host runtime**:
The interpreter or managed runtime inside a host application that executes submitted code. Its version is reported by a connected instance.
_Avoid_: Runtime without qualification.

**Host candidate**:
A discovered host application process that may support a Bridge but has not necessarily connected to the backend.
_Avoid_: Instance for a merely discovered process.

**Bridge**:
The host-side connector that registers an application with the backend and carries execution requests and results. It runs within the host process.
_Avoid_: Host application, instance.

**Bridge core**:
The shared native component of a Bridge that owns its connection, registration, heartbeat, framing, and reconnection behavior. It does not execute host code.

**Host adapter**:
The host-side code that dispatches a Bridge execution request through the host runtime on the thread required by that application.

**Backend**:
The Flint service that accepts control commands and Bridge connections, coordinates executions, and owns workflow records.
_Avoid_: Server when referring to this service as a whole.

**Connected instance**:
A transient backend registration of a Bridge in a host application, identified by an instance ID. Reconnection can produce a new instance ID for the same Bridge.
_Avoid_: Host application for the registered connection.

**Control client**:
The CLI-side component that sends a command to the backend and receives its response. It is separate from the host-side Bridge.
_Avoid_: Client without qualification.

**Control endpoint**:
The backend address and port used by control clients to send commands.

**Registry endpoint**:
The backend address and port used by Bridges for registration, heartbeat, and execution traffic.
_Avoid_: Registration-only endpoint.

**Workflow**:
A durable group of related executions with its own identifier, name, and description. Its records remain after a backend restart.

**Execution**:
One submitted code run associated with a workflow and a connected instance, together with its recorded status, output, and error information. A lost connection can leave the host outcome unknown.

**Wire protocol**:
The versioned messages and length-prefixed framing exchanged between the backend, control clients, and Bridges.
_Avoid_: Protobuf alone for the complete wire contract.
