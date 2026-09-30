import {
  Badge,
  Box,
  Button,
  Card,
  Container,
  EmptyState,
  Flex,
  Grid,
  Heading,
  Icon,
  Image,
  Link,
  PageBar,
  Section,
  Separator,
  Spinner,
  Text,
} from "@cairn/ui";
import { AppWindow, RefreshCw } from "lucide-react";
import { useState, type ReactNode } from "react";
import {
  focusApplication,
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
import { ErrorNotice, Loading, Property } from "./shared.js";
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
            <Badge tone={instance ? (instance.execution_ready ? "success" : "warning") : "neutral"}>
              {instance ? (instance.execution_ready ? "Ready" : "Connecting") : "Not connected"}
            </Badge>
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
  error: string;
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
        <ErrorNotice error={error} retry={retry} />
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
  const name = instance?.instance_name ?? (host ? hostName(host) : "Application");
  const preview = useResource(
    pid !== undefined ? `preview:${host}:${pid}` : null,
    (signal) => loadWindowPreview(pid!, signal),
    10000,
  );
  const [actionError, setActionError] = useState("");
  const focus = async () => {
    if (pid === undefined) return;
    setActionError("");
    try {
      await focusApplication(pid);
    } catch (error) {
      setActionError(messageOf(error));
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
  return (
    <>
      <PageBar.Root>
        <PageBar.Back
          label="Back to applications"
          onSelect={() => navigate("apps")}
        />
        <PageBar.Title>
          {instance?.instance_name ??
            (candidate ? hostName(candidate.host) : "Application")}
        </PageBar.Title>
        {instance || candidate ? (
          <PageBar.Action
            icon={RefreshCw}
            label="Refresh preview"
            onSelect={preview.reload}
            disabled={preview.loading}
          />
        ) : null}
      </PageBar.Root>
      <Container size="3" px="5" pb="6">
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
          <Section size="1">
            <Flex direction="column" gap="5">
              <ErrorNotice error={actionError} />
              <Image
                key={pid}
                src={preview.data?.preview?.image ?? undefined}
                alt={`Window preview of ${preview.data?.window?.title || name}`}
                aspectRatio="16/10"
                fit="contain"
                loading="eager"
                fallback={
                  <>
                    <Icon glyph={AppWindow} size="lg" />
                    <Text tone="muted">
                      {preview.loading
                        ? "Loading preview"
                        : preview.data?.preview?.unavailable_reason ||
                          "Window preview unavailable"}
                    </Text>
                  </>
                }
              />
              <ErrorNotice error={preview.error} retry={preview.reload} />
              {preview.data?.window?.title ? (
                <Property label="Window">{preview.data.window.title}</Property>
              ) : null}
              {preview.data?.window ? (
                <Box>
                  <Button onClick={() => void focus()}>
                    Switch to application
                  </Button>
                </Box>
              ) : null}
              {instance ? (
                <>
                  <Box>
                    <Badge tone={instance.execution_ready ? "success" : "warning"}>
                      {instance.execution_ready ? "Ready to execute" : "Connecting"}
                    </Badge>
                  </Box>
                  <Grid columns={{ initial: "1", sm: "2" }} gap="5">
                    <Property label="Application">
                      {hostName(instance.instance_type)}
                    </Property>
                    <Property label="Process ID">{instance.pid}</Property>
                    <Property label="Runtime">{instance.runtime_version}</Property>
                    <Property label="Bridge version">
                      {instance.bridge_version}
                    </Property>
                  </Grid>
                  <Property label="Instance ID">{instance.instance_id}</Property>
                  <Separator />
                  <Heading size="title-small">Related workflows</Heading>
                  <ErrorNotice error={related.error} retry={related.reload} />
                  {related.loading && !related.data ? (
                    <Loading />
                  ) : records.length === 0 ? (
                    <Text tone="muted">
                      No executions recorded for this connection.
                    </Text>
                  ) : (
                    <Flex direction="column" gap="3">
                      {records.map((record) => (
                        <Link
                          href={`#/${workflowPath(record.workflow_id)}`}
                          key={record.workflow_id}
                        >
                          {record.name || "Untitled workflow"}
                        </Link>
                      ))}
                    </Flex>
                  )}
                </>
              ) : null}
            </Flex>
          </Section>
        ) : null}
        {candidate ? (
          <Section size="1">
            <Flex direction="column" gap="5">
              <Property label="Process ID">{candidate.pid}</Property>
              <Property label="Executable">
                {candidate.executable || "Unavailable"}
              </Property>
              <Heading size="title-small">Connect a Bridge</Heading>
              <Text as="p">
                Load the Flint Bridge inside {hostName(candidate.host)}. Once it
                connects, the application appears in Connected.
              </Text>
            </Flex>
          </Section>
        ) : null}
      </Container>
    </>
  );
}

export function Applications({
  route,
  snapshot,
  loading,
  error,
  refresh,
  discovery,
}: Readonly<{
  route: Route;
  snapshot: Snapshot | null;
  loading: boolean;
  error: string;
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
        {error || discovery.error ? (
          <Box p="5">
            <ErrorNotice error={error || discovery.error} retry={reload} />
          </Box>
        ) : null}
      </>
    );
  }
  const firstLoad = snapshot === null || discovery.data === null;
  const pageEmpty = instances.length === 0 && candidates.length === 0 && !error && !discovery.error;
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
              error={error}
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
