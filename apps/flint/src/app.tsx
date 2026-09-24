import {
  Alert,
  AlertDialog,
  AlertTitle,
  Badge,
  BadgeDot,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  EmptyState,
  Icon,
  Link,
  PageScaffold,
  Separator,
} from "@cairn/ui";
import { useEffect, useState } from "react";

import {
  discoverHosts,
  readSnapshot,
  stopBackend,
  type ConnectedInstance,
  type HostCandidate,
  type Snapshot,
} from "./backend.js";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [serviceError, setServiceError] = useState("");
  const [actionError, setActionError] = useState("");
  const [candidates, setCandidates] = useState<readonly HostCandidate[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [stopDialogOpen, setStopDialogOpen] = useState(false);

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    const refresh = async () => {
      try {
        const next = await readSnapshot();
        if (active) {
          setSnapshot(next);
          setServiceError("");
        }
      } catch (error) {
        if (active) {
          setServiceError(messageOf(error));
        }
      } finally {
        if (active) {
          timer = window.setTimeout(refresh, 1000);
        }
      }
    };
    void refresh();
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, []);

  const scan = async () => {
    setScanning(true);
    setActionError("");
    try {
      setCandidates(await discoverHosts());
    } catch (error) {
      setActionError(messageOf(error));
    } finally {
      setScanning(false);
    }
  };

  const stop = async () => {
    setStopping(true);
    setActionError("");
    try {
      await stopBackend();
    } catch (error) {
      setActionError(messageOf(error));
      setStopping(false);
    }
  };

  const ready = snapshot?.backend.ready === true && serviceError === "";
  const count = snapshot?.instances.length ?? 0;

  return (
    <div>
      <PageScaffold
        actions={
          <Badge tone={ready ? "success" : "warning"}>
            <BadgeDot />
            {ready ? "Backend running" : snapshot === null && serviceError === "" ? "Connecting" : "Unavailable"}
          </Badge>
        }
        description="Flint keeps a bridge to your running applications. The backend stays in the system tray when this window closes."
        eyebrow="Flint · Application bridge"
        title={
          <span className="flint-title">
            <img aria-hidden="true" src="./icon.svg" />
            Connected to your work.
          </span>
        }
      >
        {serviceError !== "" || actionError !== "" ? (
          <Alert tone="destructive">
            <AlertTitle>Flint could not complete the request</AlertTitle>
            {actionError || serviceError}
          </Alert>
        ) : null}

        <section aria-labelledby="connected-title" className="flint-section">
          <div className="flint-section-heading">
            <CardTitle id="connected-title">Connected instances</CardTitle>
            <Badge tone="neutral">{count} connected</Badge>
          </div>
          {count === 0 ? (
            <EmptyState
              description="Load a Flint Bridge in Maya, 3ds Max, Blender, Unity, or Python to establish a connection."
              icon="app-window"
              title="No connected instances yet"
            />
          ) : (
            <div className="flint-list">
              {snapshot?.instances.map((instance) => (
                <InstanceCard instance={instance} key={`${instance.instance_type}:${instance.pid}:${instance.instance_name}`} />
              ))}
            </div>
          )}
        </section>

        <section aria-labelledby="discovery-title" className="flint-section">
          <Card variant="muted">
            <CardHeader>
              <CardTitle id="discovery-title">Running applications</CardTitle>
              <CardDescription>
                Discovery finds supported processes on this computer. A process appears above only after its Bridge
                connects.
              </CardDescription>
            </CardHeader>
            <CardContent>
              {candidates === null ? (
                <CardDescription>Scan to find applications that can host a Flint Bridge.</CardDescription>
              ) : candidates.length === 0 ? (
                <CardDescription>No supported applications are running.</CardDescription>
              ) : (
                <ul aria-label="Discovered applications" className="flint-candidates">
                  {candidates.map((candidate) => (
                    <li className="flint-candidate" key={`${candidate.host}:${candidate.pid}`}>
                      <Icon name="app-window" size="sm" />
                      <span className="flint-candidate-name">{candidate.host}</span>
                      <CardDescription>PID {candidate.pid}</CardDescription>
                      <Badge tone="neutral">Connect from host</Badge>
                    </li>
                  ))}
                </ul>
              )}
            </CardContent>
            <CardFooter>
              <Button loading={scanning} onClick={() => void scan()} size="sm" variant="outline">
                <Icon name="compass" size="sm" />
                Scan applications
              </Button>
            </CardFooter>
          </Card>
        </section>

        <footer className="flint-footer">
          <Separator />
          <div className="flint-footer-content">
            <div>
              <strong>Backend service</strong>
              <CardDescription>
                {snapshot === null
                  ? "Waiting for backend status"
                  : `PID ${snapshot.backend.pid} · ${snapshot.backend.registry_host}:${snapshot.backend.registry_port}`}
              </CardDescription>
            </div>
            <div className="flint-footer-actions">
              <Link href="#/legal">Licenses</Link>
              <Button disabled={!ready || stopping} onClick={() => setStopDialogOpen(true)} size="sm" variant="ghost">
                Stop backend
              </Button>
            </div>
          </div>
        </footer>
      </PageScaffold>
      <AlertDialog
        confirmLabel="Stop backend"
        description="Flint will disconnect its Bridges and close the desktop application. Running host applications remain open."
        onConfirm={() => void stop()}
        onOpenChange={setStopDialogOpen}
        open={stopDialogOpen}
        title="Stop Flint?"
      />
    </div>
  );
}

function InstanceCard({ instance }: Readonly<{ instance: ConnectedInstance }>) {
  return (
    <Card>
      <CardContent className="flint-instance">
        <Icon name="app-window" />
        <div className="flint-instance-copy">
          <h3>{instance.instance_name}</h3>
          <CardDescription>
            {instance.instance_type} · PID {instance.pid} · {instance.runtime_version}
          </CardDescription>
        </div>
        <Badge tone={instance.execution_ready ? "success" : "warning"}>
          <BadgeDot />
          {instance.execution_ready ? "Ready to execute" : "Connecting"}
        </Badge>
      </CardContent>
    </Card>
  );
}
