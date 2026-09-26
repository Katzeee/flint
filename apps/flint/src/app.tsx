import {
  Alert,
  AlertDialog,
  AlertTitle,
  Badge,
  BadgeDot,
  Box,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
  EmptyState,
  Flex,
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
        mark="./icon.svg"
        title="Connected to your work."
      >
        <Flex direction="column" gap="7">
          {serviceError !== "" || actionError !== "" ? (
            <Alert tone="destructive">
              <AlertTitle>Flint could not complete the request</AlertTitle>
              {actionError || serviceError}
            </Alert>
          ) : null}

          <Flex aria-labelledby="connected-title" as="section" direction="column" gap="3">
            <Flex align="center" gap="3" justify="between" wrap="wrap">
              <CardTitle id="connected-title">Connected instances</CardTitle>
              <Badge tone="neutral">{count} connected</Badge>
            </Flex>
            {count === 0 ? (
              <EmptyState
                description="Load a Flint Bridge in Maya, 3ds Max, Blender, Unity, or Python to establish a connection."
                icon="app-window"
                title="No connected instances yet"
              />
            ) : (
              <Flex direction="column" gap="3">
                {snapshot?.instances.map((instance) => (
                  <InstanceCard
                    instance={instance}
                    key={`${instance.instance_type}:${instance.pid}:${instance.instance_name}`}
                  />
                ))}
              </Flex>
            )}
          </Flex>

          <section aria-labelledby="discovery-title">
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
                  <Flex aria-label="Discovered applications" as="ul" direction="column" gap="3">
                    {candidates.map((candidate, index) => (
                      <Flex as="li" direction="column" gap="3" key={`${candidate.host}:${candidate.pid}`}>
                        {index === 0 ? null : <Separator />}
                        <Flex align="center" gap="3" wrap="wrap">
                          <Icon name="app-window" size="sm" />
                          <Box flexGrow="1" minWidth="0">
                            {candidate.host}
                          </Box>
                          <CardDescription>PID {candidate.pid}</CardDescription>
                          <Badge tone="neutral">Connect from host</Badge>
                        </Flex>
                      </Flex>
                    ))}
                  </Flex>
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

          <Flex as="footer" direction="column" gap="4">
            <Separator />
            <Flex align="center" gap="3" justify="between" wrap="wrap">
              <div>
                <strong>Backend service</strong>
                <CardDescription>
                  {snapshot === null
                    ? "Waiting for backend status"
                    : `PID ${snapshot.backend.pid} · ${snapshot.backend.registry_host}:${snapshot.backend.registry_port}`}
                </CardDescription>
              </div>
              <Flex align="center" gap="3">
                <Link href="#/legal">Licenses</Link>
                <Button disabled={!ready || stopping} onClick={() => setStopDialogOpen(true)} size="sm" variant="ghost">
                  Stop backend
                </Button>
              </Flex>
            </Flex>
          </Flex>
        </Flex>
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
      <CardContent>
        <Flex align="center" gap="3" wrap="wrap">
          <Icon name="app-window" />
          <Flex direction="column" flexGrow="1" gap="1" minWidth="0">
            <CardTitle as="h3" size="compact">
              {instance.instance_name}
            </CardTitle>
            <CardDescription>
              {instance.instance_type} · PID {instance.pid} · {instance.runtime_version}
            </CardDescription>
          </Flex>
          <Badge tone={instance.execution_ready ? "success" : "warning"}>
            <BadgeDot />
            {instance.execution_ready ? "Ready to execute" : "Connecting"}
          </Badge>
        </Flex>
      </CardContent>
    </Card>
  );
}
