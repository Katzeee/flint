import {
  AlertDialog,
  Badge,
  Box,
  Button,
  Callout,
  Card,
  EmptyState,
  Flex,
  Heading,
  Icon,
  Link,
  PageScaffold,
  Separator,
  Text,
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
          <Badge color={ready ? "success" : "warning"} size="2">
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
            <Callout.Root color="danger" role="alert">
              <Flex direction="column" gap="1">
                <Text as="p" weight="bold">
                  Flint could not complete the request
                </Text>
                <Callout.Text>{actionError || serviceError}</Callout.Text>
              </Flex>
            </Callout.Root>
          ) : null}

          <Flex aria-labelledby="connected-title" as="section" direction="column" gap="3">
            <Flex align="center" gap="3" justify="between" wrap="wrap">
              <Heading as="h2" id="connected-title" size="4">
                Connected instances
              </Heading>
              <Badge color="gray" size="2">
                {count} connected
              </Badge>
            </Flex>
            {count === 0 ? (
              <EmptyState>
                <EmptyState.Illustration>
                  <Icon name="app-window" size="4" />
                </EmptyState.Illustration>
                <EmptyState.Title>No connected instances yet</EmptyState.Title>
                <EmptyState.Description>
                  Load a Flint Bridge in Maya, 3ds Max, Blender, Unity, or Python to establish a connection.
                </EmptyState.Description>
              </EmptyState>
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
            <Card size="3">
              <Flex direction="column" gap="4">
                <Flex direction="column" gap="1">
                  <Heading as="h2" id="discovery-title" size="4">
                    Running applications
                  </Heading>
                  <Text as="p" color="gray" size="2">
                    Discovery finds supported processes on this computer. A process appears above only after its Bridge
                    connects.
                  </Text>
                </Flex>
                {candidates === null ? (
                  <Text as="p" color="gray" size="2">
                    Scan to find applications that can host a Flint Bridge.
                  </Text>
                ) : candidates.length === 0 ? (
                  <Text as="p" color="gray" size="2">
                    No supported applications are running.
                  </Text>
                ) : (
                  <Flex aria-label="Discovered applications" as="ul" direction="column" gap="3">
                    {candidates.map((candidate, index) => (
                      <Flex as="li" direction="column" gap="3" key={`${candidate.host}:${candidate.pid}`}>
                        {index === 0 ? null : <Separator size="4" />}
                        <Flex align="center" gap="3" wrap="wrap">
                          <Icon name="app-window" size="2" />
                          <Box flexGrow="1" minWidth="0">
                            {candidate.host}
                          </Box>
                          <Text as="p" color="gray" size="2">
                            PID {candidate.pid}
                          </Text>
                          <Badge color="gray" size="2">
                            Connect from host
                          </Badge>
                        </Flex>
                      </Flex>
                    ))}
                  </Flex>
                )}
                <Flex>
                  <Button loading={scanning} onClick={() => void scan()} size="2" variant="outline">
                    <Icon name="compass" size="2" />
                    Scan applications
                  </Button>
                </Flex>
              </Flex>
            </Card>
          </section>

          <Flex as="footer" direction="column" gap="4">
            <Separator size="4" />
            <Flex align="center" gap="3" justify="between" wrap="wrap">
              <div>
                <strong>Backend service</strong>
                <Text as="p" color="gray" size="2">
                  {snapshot === null
                    ? "Waiting for backend status"
                    : `PID ${snapshot.backend.pid} · ${snapshot.backend.registry_host}:${snapshot.backend.registry_port}`}
                </Text>
              </div>
              <Flex align="center" gap="3">
                <Link href="#/legal">Licenses</Link>
                <Button disabled={!ready || stopping} onClick={() => setStopDialogOpen(true)} size="2" variant="ghost">
                  Stop backend
                </Button>
              </Flex>
            </Flex>
          </Flex>
        </Flex>
      </PageScaffold>
      <AlertDialog.Root onOpenChange={setStopDialogOpen} open={stopDialogOpen}>
        <AlertDialog.Content>
          <AlertDialog.Title>Stop Flint?</AlertDialog.Title>
          <AlertDialog.Description>
            Flint will disconnect its Bridges and close the desktop application. Running host applications remain open.
          </AlertDialog.Description>
          <Flex gap="2" justify="end" pt="5" wrap="wrap">
            <AlertDialog.Cancel>
              <Button size="2" variant="soft">
                Cancel
              </Button>
            </AlertDialog.Cancel>
            <AlertDialog.Action>
              <Button color="danger" onClick={() => void stop()} size="2">
                Stop backend
              </Button>
            </AlertDialog.Action>
          </Flex>
        </AlertDialog.Content>
      </AlertDialog.Root>
    </div>
  );
}

function InstanceCard({ instance }: Readonly<{ instance: ConnectedInstance }>) {
  return (
    <Card size="3">
      <Flex align="center" gap="3" wrap="wrap">
        <Icon name="app-window" />
        <Flex direction="column" flexGrow="1" gap="1" minWidth="0">
          <Heading as="h3" size="3" weight="bold">
            {instance.instance_name}
          </Heading>
          <Text as="p" color="gray" size="2">
            {instance.instance_type} · PID {instance.pid} · {instance.runtime_version}
          </Text>
        </Flex>
        <Badge color={instance.execution_ready ? "success" : "warning"} size="2">
          {instance.execution_ready ? "Ready to execute" : "Connecting"}
        </Badge>
      </Flex>
    </Card>
  );
}
