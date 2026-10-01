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
import {
  attachHost,
  focusApplication,
  readDesktopInfo,
  readWorkflows,
  type ConnectedInstance,
  type HostCandidate,
  type Snapshot,
} from "./backend.js";
import {
  instancePath,
  navigate,
  workflowPath,
  type Route,
} from "./navigation.js";
import { messageOf, useResource } from "./resource.js";
import { ErrorNotice, formatTime, Loading } from "./shared.js";
import { loadWindowPreview } from "./window-preview.js";

const hostNames: Readonly<Record<string, string>> = {
  maya: "Maya",
  max: "3ds Max",
  blender: "Blender",
  unity: "Unity",
  python: "Python",
};
// eslint-disable-next-line cairn/no-raw-visual-values -- Preview tile width is a Flint layout choice.
const cardColumns = "repeat(auto-fill, min(100%, 280px))";
function hostName(host: string) {
  return hostNames[host.toLowerCase()] ?? host;
}

function ApplicationCard({
  instance,
  candidate,
  revision,
}: Readonly<{
  instance?: ConnectedInstance;
  candidate?: HostCandidate;
  revision: number;
}>) {
  const pid = instance?.pid ?? candidate!.pid;
  const host = instance?.instance_type ?? candidate!.host;
  const href = `#/${instance ? instancePath(instance.instance_id) : `apps/candidates/${pid}`}`;
  const preview = useResource(
    `preview:${host}:${pid}`,
    (signal) => loadWindowPreview(pid, signal),
    10000,
    revision,
  );
  const title = preview.data?.window?.title || instance?.instance_name || hostName(host);
  return (
    <Card as="article">
      <Card.Media>
        <Image
          src={preview.data?.preview?.image ?? undefined}
          alt=""
          aspectRatio="16/10"
          fit="contain"
          fallback={
            <>
              <Icon glyph={AppWindow} size="lg" />
              <Text size="label" tone="muted">
                {preview.loading
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
            <Card.Link href={href}>
              {title}
            </Card.Link>
          </Heading>
          <Text size="label" tone="muted">
            {hostName(host)} · PID {pid}
          </Text>
          <Flex align="center" justify="between" gap="2">
            <Status tone={instance ? (instance.execution_ready ? "success" : "warning") : "neutral"}>
              {instance ? (instance.execution_ready ? "Ready" : "Connecting") : "Not connected"}
            </Status>
            <Button size="sm" variant="ghost" onClick={() => navigate(href.slice(2))}>
              {instance ? "Details" : "Set up"}
            </Button>
          </Flex>
        </Flex>
      </Flex>
    </Card>
  );
}

function ApplicationGroup({
  title, count, ready, loading, error, retry, emptyTitle, description, children,
}: Readonly<{
  title: string;
  count: number;
  ready: boolean;
  loading: boolean;
  error?: string;
  retry: () => void;
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
        <ErrorNotice error={error ?? ""} retry={retry} />
        {count > 0 ? (
          <Grid columns={cardColumns} gap="4">{children}</Grid>
        ) : !error ? (
          <Flex direction="column" gap="1" role="status">
            <Text>{ready ? emptyTitle : "Looking for applications…"}</Text>
            <Text tone="muted">{description}</Text>
          </Flex>
        ) : null}
      </Flex>
    </Section>
  );
}

// One labelled fact in a record's summary, beside its preview.
function Fact({
  label,
  children,
}: Readonly<{ label: string; children: ReactNode }>) {
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
  instance,
  candidate,
  loading,
}: Readonly<{
  instance?: ConnectedInstance;
  candidate?: HostCandidate;
  loading: boolean;
}>) {
  const pid = instance?.pid ?? candidate?.pid;
  const host = instance?.instance_type ?? candidate?.host;
  const preview = useResource(
    pid !== undefined ? `preview:${host}:${pid}` : null,
    (signal) => loadWindowPreview(pid!, signal),
    10000,
  );
  const [actionError, setActionError] = useState("");
  const [attaching, setAttaching] = useState(false);
  const desktop = useResource("desktop-info", readDesktopInfo);
  const attachSupported = desktop.data?.attach_supported ?? false;
  const focus = async () => {
    if (pid === undefined) return;
    setActionError("");
    try {
      await focusApplication(pid);
    } catch (error) {
      setActionError(messageOf(error));
    }
  };
  const attach = async () => {
    if (pid === undefined || host === undefined) return;
    setActionError("");
    setAttaching(true);
    try {
      // The backend confirms registration; snapshot polling then reveals the
      // connected instance and this view re-renders for it.
      await attachHost(pid, host);
    } catch (error) {
      setActionError(messageOf(error));
    } finally {
      setAttaching(false);
    }
  };
  const related = useResource(
    instance ? "workflows" : null,
    readWorkflows,
    5000,
  );
  const records =
    related.data?.filter(
      (item) => instance && item.instance_ids.includes(instance.instance_id),
    ) ?? [];
  const hostWindow = preview.data?.window;
  const application = host ? hostName(host) : "Application";
  const name = instance?.instance_name || application;
  const executable = preview.data?.executable || candidate?.executable;
  const [view, setView] = useState("overview");
  return (
    <>
      <PageBar.Root>
        <PageBar.Back
          label="Back to applications"
          onSelect={() => navigate("apps")}
        />
        <PageBar.Title>
          {pid === undefined ? name : `${name} · PID ${pid}`}
        </PageBar.Title>
        {hostWindow?.title ? (
          <PageBar.Subtitle>{hostWindow.title}</PageBar.Subtitle>
        ) : null}
        {instance || candidate ? (
          <PageBar.Action
            icon={RefreshCw}
            label="Refresh preview"
            onSelect={preview.reload}
            disabled={preview.loading}
          />
        ) : null}
        {candidate && !instance && attachSupported ? (
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
        {loading && !instance && !candidate ? <Loading /> : null}
        {!loading && !instance && !candidate ? (
          <EmptyState>
            <EmptyState.Title>Application no longer available</EmptyState.Title>
            <EmptyState.Description>
              The process may have exited or its Bridge may have reconnected.
            </EmptyState.Description>
          </EmptyState>
        ) : null}
        {instance || candidate ? (
          <Flex direction="column" gap="6" pt="2">
            <Flex gap="5" wrap="wrap" align="start">
              {/* eslint-disable-next-line cairn/no-raw-visual-values -- Matches the application cards' preview width. */}
              <Box width="280px" maxWidth="100%" flexShrink="0">
                <Image
                  key={pid}
                  src={preview.data?.preview?.image ?? undefined}
                  alt={`Window preview of ${hostWindow?.title || name}`}
                  aspectRatio="16/10"
                  fit="contain"
                  loading="eager"
                  fallback={
                    <>
                      <Icon glyph={AppWindow} size="lg" />
                      <Text size="label" tone="muted">
                        {preview.loading
                          ? "Loading preview"
                          : preview.data?.preview?.unavailable_reason ||
                            "Window preview unavailable"}
                      </Text>
                    </>
                  }
                />
              </Box>
              {/* eslint-disable-next-line cairn/no-raw-visual-values -- Below this width the summary moves under the preview. */}
              <Flex direction="column" gap="4" flexGrow="1" flexBasis="320px" minWidth="0" pt="1">
                <Grid columns="2" gapX="6" gapY="4">
                  <Fact label="Bridge">
                    <Status
                      tone={
                        instance
                          ? instance.execution_ready
                            ? "success"
                            : "warning"
                          : "neutral"
                      }
                    >
                      {instance
                        ? instance.execution_ready
                          ? "Ready to execute"
                          : "Connecting"
                        : "Not connected"}
                    </Status>
                  </Fact>
                  <Fact label="Application">{application}</Fact>
                  {instance ? (
                    <>
                      <Fact label="Runtime">{instance.runtime_version}</Fact>
                      <Fact label="Bridge version">{instance.bridge_version}</Fact>
                    </>
                  ) : null}
                </Grid>
                {instance ? null : (
                  <Flex direction="column" gap="3" align="start">
                    <Text as="p" size="label" tone="muted" wrap="pretty">
                      {attachSupported
                        ? `Attach injects the Flint Bridge into ${application} so it connects without running any code inside it. You can also load the Bridge inside the application yourself.`
                        : `Load the Flint Bridge inside ${application}. Once it connects, this application moves to Connected and can run code from Flint.`}
                    </Text>
                    {candidate && attachSupported ? (
                      <Button onClick={() => void attach()} disabled={attaching}>
                        {attaching ? (
                          <Spinner size="sm" />
                        ) : (
                          <Icon glyph={Zap} size="sm" />
                        )}
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
                    {instance ? (
                      <List.Item trailing={<Code>{instance.instance_id}</Code>}>
                        Instance ID
                      </List.Item>
                    ) : null}
                    <List.Item trailing={pid}>Process ID</List.Item>
                    {hostWindow?.title ? (
                      <List.Item description={hostWindow.title}>Window</List.Item>
                    ) : null}
                    {executable ? (
                      <List.Item description={executable}>Executable</List.Item>
                    ) : null}
                  </List.Section>
                </Flex>
              </Tabs.Content>

              <Tabs.Content value="console">
                {instance ? (
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
                      The console runs code inside {application} once its Bridge
                      connects.
                    </EmptyState.Description>
                  </EmptyState>
                )}
              </Tabs.Content>

              <Tabs.Content value="workflows">
                {instance ? (
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
                          {related.loading && !related.data
                            ? "Loading workflows…"
                            : "No workflows yet"}
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
  discovery,
}: Readonly<{
  route: Route;
  snapshot: Snapshot | null;
  loading: boolean;
  refresh: () => void;
  discovery: Readonly<{
    data: readonly HostCandidate[] | null;
    loading: boolean;
    error: string;
    reload: () => void;
  }>;
}>) {
  const [previewRevision, setPreviewRevision] = useState(0);
  const instances = snapshot?.instances ?? [];
  const connectedPids = new Set(instances.map((instance) => instance.pid));
  const candidates = (discovery.data ?? []).filter(
    (candidate) => !connectedPids.has(candidate.pid),
  );
  const reload = () => {
    setPreviewRevision((value) => value + 1);
    discovery.reload();
    refresh();
  };
  if (route.id) {
    const instance = route.candidate
      ? instances.find((item) => item.pid === Number(route.id))
      : instances.find((item) => item.instance_id === route.id);
    const candidate =
      !instance && route.candidate
        ? candidates.find((item) => item.pid === Number(route.id))
        : undefined;
    return (
      <>
        <ApplicationDetail
          key={instance?.pid ?? candidate?.pid ?? route.id}
          instance={instance}
          candidate={candidate}
          loading={loading || discovery.loading}
        />
        {/* The shell's banner reports a backend that cannot be reached. */}
        {discovery.error ? (
          <Box p="5">
            <ErrorNotice error={discovery.error} retry={reload} />
          </Box>
        ) : null}
      </>
    );
  }
  const firstLoad = snapshot === null || discovery.data === null;
  const pageEmpty = instances.length === 0 && candidates.length === 0 && !discovery.error;
  return (
    <>
      <PageBar.Root>
        <PageBar.Title>Applications</PageBar.Title>
        <PageBar.Action
          icon={RefreshCw}
          label="Refresh applications"
          onSelect={reload}
          disabled={loading || discovery.loading}
        />
      </PageBar.Root>
      <Container size="4" px="5" pb="6">
        {pageEmpty ? (
          <Section size="1">
            <EmptyState role="status">
              <EmptyState.Illustration>
                {firstLoad ? <Spinner size="sm" /> : <Icon glyph={AppWindow} size="lg" />}
              </EmptyState.Illustration>
              <EmptyState.Title>{firstLoad ? "Looking for applications…" : "No applications running"}</EmptyState.Title>
              <EmptyState.Description>
                Open Blender, Maya, 3ds Max or Unity to get started.
              </EmptyState.Description>
            </EmptyState>
          </Section>
        ) : (
          <>
            <ApplicationGroup
              title="Connected"
              count={instances.length}
              ready={snapshot !== null}
              loading={loading}
              retry={refresh}
              emptyTitle="No connected applications"
              description="Choose an application below to set up its Bridge."
            >
              {instances.map((instance) => (
                <ApplicationCard key={instance.pid} instance={instance} revision={previewRevision} />
              ))}
            </ApplicationGroup>
            <ApplicationGroup
              title="Available to connect"
              count={candidates.length}
              ready={discovery.data !== null}
              loading={discovery.loading}
              error={discovery.error}
              retry={discovery.reload}
              emptyTitle="No other applications found"
              description="Open another supported application and it will appear here."
            >
              {candidates.map((candidate) => (
                <ApplicationCard key={candidate.pid} candidate={candidate} revision={previewRevision} />
              ))}
            </ApplicationGroup>
          </>
        )}
      </Container>
    </>
  );
}
