import {
  AlertDialog,
  Button,
  Code,
  Container,
  Flex,
  List,
  PageBar,
  Select,
  Status,
  type CairnAppearance,
} from "@cairn/ui";
import { useState } from "react";
import { type BackendStatus, readBridgeToken, readDesktopInfo } from "./backend.js";
import { useResource } from "./resource.js";
import { ErrorNotice } from "./shared.js";

export function Settings({
  appearance,
  onAppearanceChange,
  preferenceError,
  backend,
  busy,
  error,
  changeBackend,
}: Readonly<{
  appearance: CairnAppearance;
  onAppearanceChange: (appearance: CairnAppearance) => void;
  preferenceError: string;
  backend: BackendStatus | null;
  busy: "start" | "stop" | null;
  error: string;
  changeBackend: (operation: "start" | "stop") => Promise<void>;
}>) {
  const info = useResource("desktop-info", readDesktopInfo);
  const ready = backend !== null;
  const [confirmStop, setConfirmStop] = useState(false);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  const pending = info.loading && !info.data ? "Loading…" : "Unavailable";
  const copyToken = async () => {
    try {
      await navigator.clipboard.writeText(await readBridgeToken());
      setCopyError("");
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      setCopyError(error instanceof Error ? error.message : String(error));
    }
  };
  return (
    <>
      <PageBar.Root>
        <PageBar.Title>Settings</PageBar.Title>
      </PageBar.Root>
      <Container size="2" px="5" pb="6">
        <Flex direction="column" gap="6" pt="2">
          <List.Section title="Appearance">
            <List.Item
              control={
                <Select.Root value={appearance} onValueChange={(value) => onAppearanceChange(value as CairnAppearance)}>
                  <Select.Trigger />
                  <Select.Content>
                    <Select.Item value="inherit">System</Select.Item>
                    <Select.Item value="light">Light</Select.Item>
                    <Select.Item value="dark">Dark</Select.Item>
                  </Select.Content>
                </Select.Root>
              }
            >
              Theme
            </List.Item>
            <List.Item
              description="The desktop stays in the system tray. The backend runs independently."
              trailing="Always"
            >
              Closing the window
            </List.Item>
          </List.Section>
          <ErrorNotice error={preferenceError} />

          <List.Section title="Backend">
            <List.Item
              trailing={
                <Flex align="center" gap="3">
                  <Status tone={ready ? "success" : "neutral"}>{ready ? "Running" : "Not running"}</Status>
                  {ready ? (
                    <Button
                      disabled={busy !== null}
                      loading={busy === "stop"}
                      size="sm"
                      variant="secondary"
                      onClick={() => setConfirmStop(true)}
                    >
                      Stop
                    </Button>
                  ) : (
                    <Button
                      disabled={busy !== null}
                      loading={busy === "start"}
                      size="sm"
                      onClick={() => void changeBackend("start")}
                    >
                      Start
                    </Button>
                  )}
                </Flex>
              }
            >
              Status
            </List.Item>
            <List.Item trailing={info.data ? <Code>{info.data.control_endpoint}</Code> : pending}>
              Control endpoint
            </List.Item>
            <List.Item trailing={info.data?.state_dir ?? pending}>Data directory</List.Item>
          </List.Section>
          <ErrorNotice error={error} />
          <ErrorNotice error={info.error} retry={info.reload} />

          <List.Section
            title="Remote Bridges"
            description="On another machine, enter one of these addresses, the port, and the token in the Bridge's connection settings."
          >
            {backend ? (
              backend.bridge_addresses.length > 0 ? (
                backend.bridge_addresses.map((network) => (
                  <List.Item key={`${network.interface}/${network.address}`} trailing={<Code>{network.address}</Code>}>
                    {network.interface}
                  </List.Item>
                ))
              ) : (
                <List.Item trailing="None">Network addresses</List.Item>
              )
            ) : (
              <List.Item trailing="Backend not running">Network addresses</List.Item>
            )}
            <List.Item trailing={backend ? <Code>{backend.bridge_port}</Code> : "—"}>Port</List.Item>
            <List.Item
              description="Bridges on this machine use it automatically."
              trailing={
                <Button disabled={!ready} size="sm" variant="secondary" onClick={() => void copyToken()}>
                  {copied ? "Copied" : "Copy"}
                </Button>
              }
            >
              Token
            </List.Item>
          </List.Section>
          <ErrorNotice error={copyError} />

          <List.Section title="About">
            <List.Item trailing={info.data?.version ?? pending}>Version</List.Item>
            <List.Item href="#/legal">Open-source licenses</List.Item>
          </List.Section>
        </Flex>
      </Container>
      <AlertDialog.Root open={confirmStop} onOpenChange={setConfirmStop}>
        <AlertDialog.Content>
          <AlertDialog.Title>Stop backend?</AlertDialog.Title>
          <AlertDialog.Description>
            Every Bridge disconnects, and any running execution is interrupted.
          </AlertDialog.Description>
          <AlertDialog.Actions>
            <AlertDialog.Cancel>
              <Button variant="secondary">Cancel</Button>
            </AlertDialog.Cancel>
            <AlertDialog.Action>
              <Button variant="destructive" disabled={busy !== null} onClick={() => void changeBackend("stop")}>
                Stop backend
              </Button>
            </AlertDialog.Action>
          </AlertDialog.Actions>
        </AlertDialog.Content>
      </AlertDialog.Root>
    </>
  );
}
