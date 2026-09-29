import {
  AlertDialog,
  Box,
  Button,
  Container,
  Flex,
  Heading,
  Link,
  PageBar,
  Section,
  Select,
  Separator,
  Text,
  type CairnAppearance,
} from "@cairn/ui";
import { useState } from "react";
import { readDesktopInfo, stopBackend } from "./backend.js";
import { messageOf, useResource } from "./resource.js";
import { ErrorNotice, Loading, Property } from "./shared.js";

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
  return (
    <>
      <PageBar.Root>
        <PageBar.Title>Settings</PageBar.Title>
      </PageBar.Root>
      <Container size="2" px="5" pb="6">
        <ErrorNotice error={preferenceError || error || info.error} />
        <Section size="1">
          <Flex direction="column" gap="4">
            <Heading size="title-small">Appearance</Heading>
            <Flex align="center" justify="between" gap="4" wrap="wrap">
              <Text>Theme</Text>
              <Select.Root
                value={appearance}
                onValueChange={(value) =>
                  onAppearanceChange(value as CairnAppearance)
                }
              >
                <Select.Trigger aria-label="Theme" />
                <Select.Content>
                  <Select.Item value="inherit">System</Select.Item>
                  <Select.Item value="light">Light</Select.Item>
                  <Select.Item value="dark">Dark</Select.Item>
                </Select.Content>
              </Select.Root>
            </Flex>
          </Flex>
        </Section>
        <Separator />
        <Section size="1">
          <Flex direction="column" gap="3">
            <Heading size="title-small">Window behavior</Heading>
            <Text as="p" tone="muted">
              Closing the window keeps Flint and its connections running in the
              system tray.
            </Text>
          </Flex>
        </Section>
        <Separator />
        <Section size="1">
          <Flex direction="column" gap="4">
            <Heading size="title-small">Backend</Heading>
            {info.loading && !info.data ? <Loading /> : null}
            {info.data ? (
              <>
                <Property label="Control endpoint">
                  {info.data.control_endpoint}
                </Property>
                <Property label="Bridge endpoint">
                  {info.data.registry_endpoint}
                </Property>
                <Property label="Data directory">
                  {info.data.state_dir}
                </Property>
              </>
            ) : null}
            <Box>
              <Button
                disabled={!ready || stopping}
                loading={stopping}
                variant="outline"
                onClick={() => setConfirmStop(true)}
              >
                Stop backend
              </Button>
            </Box>
          </Flex>
        </Section>
        <Separator />
        <Section size="1">
          <Flex direction="column" gap="3">
            <Heading size="title-small">About Flint</Heading>
            <Text tone="muted">
              {info.data ? `Version ${info.data.version}` : "Flint"}
            </Text>
            <Link href="#/legal">Open-source licenses</Link>
          </Flex>
        </Section>
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
