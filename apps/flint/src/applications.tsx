import {
  Badge,
  Box,
  Button,
  Card,
  Code,
  Container,
  EmptyState,
  Flex,
  Grid,
  Heading,
  Icon,
  Image,
  List,
  PageBar,
  Section,
  Spinner,
  Status,
  Tabs,
  Text,
  TextArea,
} from "@cairn/ui";
import { AppWindow, Play, RefreshCw, SquareArrowOutUpRight, Zap } from "lucide-react";
import { useState, type ReactNode } from "react";
import { attachApp, focusApp, readWorkflows, type App, type AppInfo, type Snapshot } from "./backend.js";
import { appPath, navigate, workflowPath, type Route } from "./navigation.js";
import { messageOf, useResource } from "./resource.js";
import { ErrorNotice, formatTime, Loading } from "./shared.js";
import { loadWindowPreview } from "./window-preview.js";
import { hostNames as generatedHostNames } from "./generated/bindings.js";

const hostNames: Readonly<Record<string, string>> = generatedHostNames;
// eslint-disable-next-line cairn/no-raw-visual-values -- Preview tile width is a Flint layout choice.
const cardColumns = "repeat(auto-fill, min(100%, 280px))";

type Connection = "ready" | "connecting" | "not_connected";

function connectionOf({ registration }: App): Connection {
  if (!registration) return "not_connected";
  return registration.execution_ready ? "ready" : "connecting";
}

const connectionTone = {
  ready: "success",
  connecting: "warning",
  not_connected: "neutral",
} as const;
const remotePreviewReason = "Running on another machine";

function previewImage(info: AppInfo | null | undefined) {
  const png = info?.preview?.png;
  return png ? `data:image/png;base64,${png}` : undefined;
}

function useWindowPreview(app: App | undefined, revision = 0) {
  return useResource(
    app?.local ? `preview:${app.host}:${app.pid}` : null,
    (signal) => loadWindowPreview(app!.pid, signal),
    10000,
    revision,
  );
}

function ApplicationCard({ app, revision }: Readonly<{ app: App; revision: number }>) {
  const href = `#/${appPath(app.instance_id)}`;
  const preview = useWindowPreview(app, revision);
  const title = preview.data?.window?.title || app.registration?.name || (hostNames[app.host] ?? app.host);
  const connection = connectionOf(app);
  return (
    <Card as="article">
      <Card.Media>
        <Image
          src={previewImage(preview.data)}
          alt=""
          aspectRatio="16/10"
          fit="contain"
          fallback={
            <>
              <Icon glyph={AppWindow} size="lg" />
              <Text size="label" tone="muted">
                {!app.local
                  ? remotePreviewReason
                  : preview.loading
                    ? "Loading preview"
                    : preview.data?.preview?.unavailable_reason || preview.error || "Window preview unavailable"}
              </Text>
            </>
          }
        />
      </Card.Media>
      <Flex direction="column" gap="4" minWidth="0">
        <Flex direction="column" gap="2">
          <Heading as="h3" size="body-large" truncate title={title}>
            <Card.Link href={href}>{title}</Card.Link>
          </Heading>
          <Text size="label" tone="muted">
            {hostNames[app.host] ?? app.host} · PID {app.pid}
          </Text>
          <Flex align="center" justify="between" gap="2">
            <Status tone={connectionTone[connection]}>
              {{ ready: "Ready", connecting: "Connecting", not_connected: "Not connected" }[connection]}
            </Status>
            <Button size="sm" variant="ghost" onClick={() => navigate(href.slice(2))}>
              {app.registration ? "Details" : "Set up"}
            </Button>
          </Flex>
        </Flex>
      </Flex>
    </Card>
  );
}

function ApplicationGroup({
  title,
  count,
  ready,
  loading,
  emptyTitle,
  description,
  children,
}: Readonly<{
  title: string;
  count: number;
  ready: boolean;
  loading: boolean;
  emptyTitle: string;
  description: string;
  children: ReactNode;
}>) {
  return (
    <Section size="1">
      <Flex direction="column" gap="3">
        <Flex align="center" gap="3">
          <Heading size="title-small">{title}</Heading>
          {ready ? <Badge>{count}</Badge> : null}
          {loading ? <Spinner size="sm" aria-label={`Updating ${title.toLowerCase()}`} /> : null}
        </Flex>
        {count > 0 ? (
          <Grid columns={cardColumns} gap="4">
            {children}
          </Grid>
        ) : (
          <Flex direction="column" gap="1" role="status">
            <Text>{ready ? emptyTitle : "Looking for applications…"}</Text>
            <Text tone="muted">{description}</Text>
          </Flex>
        )}
      </Flex>
    </Section>
  );
}

// One labelled fact in a record's summary, beside its preview.
function Fact({ label, children }: Readonly<{ label: string; children: ReactNode }>) {
  return (
    <Flex direction="column" gap="1" minWidth="0">
      <Text size="label" tone="muted">
        {label}
      </Text>
      <Text wrap="pretty">{children}</Text>
    </Flex>
  );
}

function ApplicationDetail({
  app,
  loading,
  onAttached,
}: Readonly<{
  app?: App;
  loading: boolean;
  onAttached: (app: App) => void;
}>) {
  const preview = useWindowPreview(app);
  const [actionError, setActionError] = useState("");
  const [attaching, setAttaching] = useState(false);
  const registration = app?.registration;
  const connection = app ? connectionOf(app) : "not_connected";
  const connected = Boolean(registration);
  const attachable = !registration && Boolean(app?.candidate?.attachable);
  const focus = async () => {
    if (app === undefined) return;
    setActionError("");
    try {
      await focusApp(app.pid);
    } catch (error) {
      setActionError(messageOf(error));
    }
  };
  const attach = async () => {
    if (app === undefined) return;
    setActionError("");
    setAttaching(true);
    try {
      onAttached(await attachApp(app.pid));
    } catch (error) {
      setActionError(messageOf(error));
    } finally {
      setAttaching(false);
    }
  };
  const related = useResource(connected ? "workflows" : null, readWorkflows, 5000);
  const records = related.data?.filter((item) => app && item.instance_ids.includes(app.instance_id)) ?? [];
  const hostWindow = preview.data?.window;
  const application = app ? (hostNames[app.host] ?? app.host) : "Application";
  const name = registration?.name || application;
  const executable = app?.candidate?.executable;
  const [view, setView] = useState("overview");
  return (
    <>
      <PageBar.Root>
        <PageBar.Back label="Back to applications" onSelect={() => navigate("apps")} />
        <PageBar.Title>{app === undefined ? name : `${name} · PID ${app.pid}`}</PageBar.Title>
        {hostWindow?.title ? <PageBar.Subtitle>{hostWindow.title}</PageBar.Subtitle> : null}
        {app?.local ? (
          <PageBar.Action
            icon={RefreshCw}
            label="Refresh preview"
            onSelect={preview.reload}
            disabled={preview.loading}
          />
        ) : null}
        {attachable ? (
          <PageBar.Action
            icon={Zap}
            label="Attach Bridge"
            onSelect={() => void attach()}
            priority="primary"
            disabled={attaching}
          />
        ) : null}
        {hostWindow ? (
          <PageBar.Action
            icon={SquareArrowOutUpRight}
            label="Switch to application"
            onSelect={() => void focus()}
            priority="primary"
          />
        ) : null}
      </PageBar.Root>
      <Container size="4" px="5" pb="6">
        {loading && !app ? <Loading /> : null}
        {!loading && !app ? (
          <EmptyState>
            <EmptyState.Title>Application no longer available</EmptyState.Title>
            <EmptyState.Description>The process may have exited.</EmptyState.Description>
          </EmptyState>
        ) : null}
        {app ? (
          <Flex direction="column" gap="6" pt="2">
            <Flex gap="5" wrap="wrap" align="start">
              {/* eslint-disable-next-line cairn/no-raw-visual-values -- Matches the application cards' preview width. */}
              <Box width="280px" maxWidth="100%" flexShrink="0">
                <Image
                  key={app.instance_id}
                  src={previewImage(preview.data)}
                  alt={`Window preview of ${hostWindow?.title || name}`}
                  aspectRatio="16/10"
                  fit="contain"
                  loading="eager"
                  fallback={
                    <>
                      <Icon glyph={AppWindow} size="lg" />
                      <Text size="label" tone="muted">
                        {!app.local
                          ? remotePreviewReason
                          : preview.loading
                            ? "Loading preview"
                            : preview.data?.preview?.unavailable_reason || "Window preview unavailable"}
                      </Text>
                    </>
                  }
                />
              </Box>
              {/* eslint-disable-next-line cairn/no-raw-visual-values -- Below this width the summary moves under the preview. */}
              <Flex direction="column" gap="4" flexGrow="1" flexBasis="320px" minWidth="0" pt="1">
                <Grid columns="2" gapX="6" gapY="4">
                  <Fact label="Bridge">
                    <Status tone={connectionTone[connection]}>
                      {
                        {
                          ready: "Ready to execute",
                          connecting: "Connecting",
                          not_connected: "Not connected",
                        }[connection]
                      }
                    </Status>
                  </Fact>
                  <Fact label="Application">{application}</Fact>
                  {registration ? (
                    <>
                      <Fact label="Runtime">{registration.runtime_version}</Fact>
                      <Fact label="Bridge version">{registration.bridge_version}</Fact>
                    </>
                  ) : null}
                </Grid>
                {connected ? null : (
                  <Flex direction="column" gap="3" align="start">
                    <Text as="p" size="label" tone="muted" wrap="pretty">
                      {attachable
                        ? `Attach injects the Flint Bridge into ${application} so it connects without running any code inside it. You can also load the Bridge inside the application yourself.`
                        : `Load the Flint Bridge inside ${application}. Once it connects, this application moves to Connected and can run code from Flint.`}
                    </Text>
                    {attachable ? (
                      <Button onClick={() => void attach()} disabled={attaching}>
                        {attaching ? <Spinner size="sm" /> : <Icon glyph={Zap} size="sm" />}
                        {attaching ? "Attaching…" : "Attach Bridge"}
                      </Button>
                    ) : null}
                  </Flex>
                )}
              </Flex>
            </Flex>
            <ErrorNotice error={preview.error} retry={preview.reload} />
            <ErrorNotice error={actionError} />

            <Tabs.Root value={view} onValueChange={setView}>
              <Tabs.List aria-label="Application views">
                <Tabs.Trigger value="overview">Overview</Tabs.Trigger>
                <Tabs.Trigger value="console">Console</Tabs.Trigger>
                <Tabs.Trigger value="workflows">Workflows</Tabs.Trigger>
              </Tabs.List>

              <Tabs.Content value="overview">
                <Flex direction="column" gap="6" pt="2">
                  <List.Section title="Details">
                    {connected ? <List.Item trailing={<Code>{app.instance_id}</Code>}>Instance ID</List.Item> : null}
                    <List.Item trailing={app.pid}>Process ID</List.Item>
                    {hostWindow?.title ? <List.Item description={hostWindow.title}>Window</List.Item> : null}
                    {executable ? <List.Item description={executable}>Executable</List.Item> : null}
                  </List.Section>
                </Flex>
              </Tabs.Content>

              <Tabs.Content value="console">
                {connected ? (
                  <Flex direction="column" gap="4" pt="2">
                    <TextArea
                      aria-label="Code to run"
                      monospaced
                      placeholder={`# Runs inside ${application} on its main thread\n`}
                      rows={10}
                      spellCheck={false}
                    />
                    <Flex align="center" justify="between" gap="3" wrap="wrap">
                      <Text size="label" tone="muted">
                        Running code from the desktop is not available yet.
                      </Text>
                      <Button disabled>
                        <Icon glyph={Play} size="sm" />
                        Run
                      </Button>
                    </Flex>
                    <Flex direction="column" gap="2">
                      <Heading as="h2" size="label">
                        Output
                      </Heading>
                      <Code aria-label="Output" block>
                        {"Output from the last run appears here."}
                      </Code>
                    </Flex>
                  </Flex>
                ) : (
                  <EmptyState>
                    <EmptyState.Title>Connect the Bridge to run code</EmptyState.Title>
                    <EmptyState.Description>
                      The console runs code inside {application} once its Bridge connects.
                    </EmptyState.Description>
                  </EmptyState>
                )}
              </Tabs.Content>

              <Tabs.Content value="workflows">
                {connected ? (
                  <Flex direction="column" gap="3" pt="2">
                    {records.length > 0 ? (
                      <List.Section>
                        {records.map((record) => (
                          <List.Item
                            key={record.workflow_id}
                            href={`#/${workflowPath(record.workflow_id)}`}
                            description={`${record.execution_count} executions · ${formatTime(record.updated_at)}`}
                          >
                            {record.name || "Untitled workflow"}
                          </List.Item>
                        ))}
                      </List.Section>
                    ) : (
                      <EmptyState>
                        <EmptyState.Title>
                          {related.loading && !related.data ? "Loading workflows…" : "No workflows yet"}
                        </EmptyState.Title>
                        <EmptyState.Description>
                          Workflows that run code in this application appear here.
                        </EmptyState.Description>
                      </EmptyState>
                    )}
                    <ErrorNotice error={related.error} retry={related.reload} />
                  </Flex>
                ) : (
                  <EmptyState>
                    <EmptyState.Title>No workflows yet</EmptyState.Title>
                    <EmptyState.Description>
                      Workflows can use this application once its Bridge connects.
                    </EmptyState.Description>
                  </EmptyState>
                )}
              </Tabs.Content>
            </Tabs.Root>
          </Flex>
        ) : null}
      </Container>
    </>
  );
}

export function Applications({
  route,
  snapshot,
  loading,
  refresh,
  onAttached,
}: Readonly<{
  route: Route;
  snapshot: Snapshot | null;
  loading: boolean;
  refresh: () => void;
  onAttached: (app: App) => void;
}>) {
  const [previewRevision, setPreviewRevision] = useState(0);
  const apps = snapshot?.apps ?? [];
  const connected = apps.filter((app) => app.registration);
  const available = apps.filter((app) => !app.registration);
  const reload = () => {
    setPreviewRevision((value) => value + 1);
    refresh();
  };
  if (route.id) {
    return (
      <ApplicationDetail
        key={route.id}
        app={apps.find((app) => app.instance_id === route.id)}
        loading={loading}
        onAttached={onAttached}
      />
    );
  }
  const firstLoad = snapshot === null;
  const pageEmpty = apps.length === 0;
  return (
    <>
      <PageBar.Root>
        <PageBar.Title>Applications</PageBar.Title>
        <PageBar.Action icon={RefreshCw} label="Refresh applications" onSelect={reload} disabled={loading} />
      </PageBar.Root>
      <Container size="4" px="5" pb="6">
        {pageEmpty ? (
          <Section size="1">
            <EmptyState role="status">
              <EmptyState.Illustration>
                {firstLoad ? <Spinner size="sm" /> : <Icon glyph={AppWindow} size="lg" />}
              </EmptyState.Illustration>
              <EmptyState.Title>{firstLoad ? "Looking for applications…" : "No applications running"}</EmptyState.Title>
              <EmptyState.Description>Open a supported application to get started.</EmptyState.Description>
            </EmptyState>
          </Section>
        ) : (
          <>
            <ApplicationGroup
              title="Connected"
              count={connected.length}
              ready={!firstLoad}
              loading={loading}
              emptyTitle="No connected applications"
              description="Choose an application below to set up its Bridge."
            >
              {connected.map((app) => (
                <ApplicationCard key={app.instance_id} app={app} revision={previewRevision} />
              ))}
            </ApplicationGroup>
            <ApplicationGroup
              title="Available to connect"
              count={available.length}
              ready={!firstLoad}
              loading={loading}
              emptyTitle="No other applications found"
              description="Open another supported application and it will appear here."
            >
              {available.map((app) => (
                <ApplicationCard key={app.instance_id} app={app} revision={previewRevision} />
              ))}
            </ApplicationGroup>
          </>
        )}
      </Container>
    </>
  );
}
