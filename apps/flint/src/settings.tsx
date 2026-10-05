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
import { readDesktopInfo, stopBackend } from "./backend.js";
import { messageOf, useResource } from "./resource.js";
import { ErrorNotice } from "./shared.js";

export function Settings({
  appearance,
  onAppearanceChange,
  preferenceError,
  ready,
}: Readonly<{
  appearance: CairnAppearance;
  onAppearanceChange: (appearance: CairnAppearance) => void;
  preferenceError: string;
  ready: boolean;
}>) {
  const info = useResource("desktop-info", readDesktopInfo);
  const [confirmStop, setConfirmStop] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const stop = async () => {
    setStopping(true);
    setError("");
    try {
      await stopBackend();
    } catch (failure) {
      setError(messageOf(failure));
      setStopping(false);
    }
  };
  const pending = info.loading && !info.data ? "Loading…" : "Unavailable";
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
                <Select.Root
                  value={appearance}
                  onValueChange={(value) =>
                    onAppearanceChange(value as CairnAppearance)
                  }
                >
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
              description="Flint and its connections keep running in the system tray."
              trailing="Always"
            >
              Closing the window
            </List.Item>
          </List.Section>
          <ErrorNotice error={preferenceError} />

          <List.Section title="Backend">
            <List.Item
              trailing={
                <Status tone={ready ? "success" : "neutral"}>
                  {ready ? "Running" : "Not running"}
                </Status>
              }
            >
              Status
            </List.Item>
            <List.Item trailing={info.data ? <Code>{info.data.control_endpoint}</Code> : pending}>
              Control endpoint
            </List.Item>
            <List.Item trailing={info.data ? <Code>{info.data.bridge_endpoint}</Code> : pending}>
              Bridge endpoint
            </List.Item>
            <List.Item trailing={info.data?.state_dir ?? pending}>
              Data directory
            </List.Item>
          </List.Section>
          <ErrorNotice error={info.error} retry={info.reload} />

          <List.Section description="Running host applications remain open and reconnect when Flint starts again.">
            <List.Item
              description="Disconnects every Bridge and closes Flint."
              trailing={
                <Button
                  disabled={!ready || stopping}
                  loading={stopping}
                  size="sm"
                  variant="destructive"
                  onClick={() => setConfirmStop(true)}
                >
                  Stop
                </Button>
              }
            >
              Stop backend
            </List.Item>
          </List.Section>
          <ErrorNotice error={error} />

          <List.Section title="About">
            <List.Item trailing={info.data?.version ?? pending}>Version</List.Item>
            <List.Item href="#/legal">Open-source licenses</List.Item>
          </List.Section>
        </Flex>
      </Container>
      <AlertDialog.Root open={confirmStop} onOpenChange={setConfirmStop}>
        <AlertDialog.Content>
          <AlertDialog.Title>Stop Flint?</AlertDialog.Title>
          <AlertDialog.Description>
            Flint will disconnect its Bridges and close the desktop application.
            Running host applications remain open.
          </AlertDialog.Description>
          <AlertDialog.Actions>
            <AlertDialog.Cancel>
              <Button variant="secondary">Cancel</Button>
            </AlertDialog.Cancel>
            <AlertDialog.Action>
              <Button variant="destructive" onClick={() => void stop()}>
                Stop backend
              </Button>
            </AlertDialog.Action>
          </AlertDialog.Actions>
        </AlertDialog.Content>
      </AlertDialog.Root>
    </>
  );
}
