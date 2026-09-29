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
  Text,
} from "@cairn/ui";
import { useState } from "react";
import {
  discoverHosts,
  focusInstance,
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
function hostName(host: string) {
  return hostNames[host.toLowerCase()] ?? host;
}

function InstanceCard({
  instance,
  revision,
}: Readonly<{ instance: ConnectedInstance; revision: number }>) {
  const preview = useResource(
    `preview:${instance.instance_id}`,
    (signal) => loadWindowPreview(instance.instance_id, signal),
    10000,
    revision,
  );
  const title = preview.data?.title || instance.instance_name;
  return (
    <Card as="article">
      <Card.Media>
        <Image
          src={preview.data?.image ?? undefined}
          alt=""
          aspectRatio="16/10"
          fit="contain"
          fallback={
            <>
              <Icon name="app-window" size="lg" />
              <Text size="label" tone="muted">
                {preview.loading
                  ? "Loading preview"
                  : "Window preview unavailable"}
              </Text>
            </>
          }
        />
      </Card.Media>
      <Flex direction="column" gap="4" minWidth="0">
        <Flex direction="column" gap="2">
          <Heading as="h3" size="body-large">
            <Card.Link href={`#/${instancePath(instance.instance_id)}`}>
              {title}
            </Card.Link>
          </Heading>
          <Text size="label" tone="muted">
            {hostName(instance.instance_type)} · PID {instance.pid}
          </Text>
          <Box>
            <Badge tone={instance.execution_ready ? "success" : "warning"}>
              {instance.execution_ready ? "Ready to execute" : "Connecting"}
            </Badge>
          </Box>
        </Flex>
      </Flex>
    </Card>
  );
}

function CandidateCard({ candidate }: Readonly<{ candidate: HostCandidate }>) {
  return (
    <Card as="article">
      <Flex direction="column" gap="3">
        <Heading as="h3" size="body-large">
          <Card.Link href={`#/apps/candidates/${candidate.pid}`}>
            {hostName(candidate.host)}
          </Card.Link>
        </Heading>
        <Text size="label" tone="muted">
          PID {candidate.pid}
        </Text>
        <Text size="label">
          {candidate.attach_supported
            ? "Injection supported"
            : "Connect from host"}
        </Text>
      </Flex>
    </Card>
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
  const preview = useResource(
    instance ? `preview:${instance.instance_id}` : null,
    (signal) => loadWindowPreview(instance!.instance_id, signal),
    10000,
  );
  const [actionError, setActionError] = useState("");
  const focus = async () => {
    if (!instance) return;
    setActionError("");
    try {
      await focusInstance(instance.instance_id);
    } catch (error) {
      setActionError(messageOf(error));
    }
  };
  const related = useResource(
    instance ? `related:${instance.instance_id}` : null,
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
        {instance ? (
          <PageBar.Action
            icon="compass"
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
        {instance ? (
          <Section size="1">
            <Flex direction="column" gap="5">
              <ErrorNotice error={actionError} />
              <Image
                key={instance.instance_id}
                src={preview.data?.image ?? undefined}
                alt={`Window preview of ${preview.data?.title || instance.instance_name}`}
                aspectRatio="16/10"
                fit="contain"
                loading="eager"
                fallback={
                  <>
                    <Icon name="app-window" size="lg" />
                    <Text tone="muted">
                      {preview.loading
                        ? "Loading preview"
                        : preview.data?.unavailable_reason ||
                          "Window preview unavailable"}
                    </Text>
                  </>
                }
              />
              <ErrorNotice error={preview.error} retry={preview.reload} />
              {preview.data?.title ? (
                <Property label="Window">{preview.data.title}</Property>
              ) : null}
              {preview.data?.can_focus ? (
                <Box>
                  <Button onClick={() => void focus()}>
                    Switch to application
                  </Button>
                </Box>
              ) : null}
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
              {candidate.attach_supported ? (
                <Text tone="muted">
                  Injection is not available in this desktop build.
                </Text>
              ) : null}
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
}: Readonly<{
  route: Route;
  snapshot: Snapshot | null;
  loading: boolean;
  error: string;
  refresh: () => void;
}>) {
  const discovery = useResource("candidates", discoverHosts, 10000);
  const [previewRevision, setPreviewRevision] = useState(0);
  const instances = snapshot?.instances ?? [];
  const connectedPids = new Set(instances.map((instance) => instance.pid));
  const candidates = (discovery.data ?? []).filter(
    (candidate) => !connectedPids.has(candidate.pid),
  );
  const injectable = candidates.filter(
    (candidate) => candidate.attach_supported,
  );
  const manual = candidates.filter((candidate) => !candidate.attach_supported);
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
          key={instance?.instance_id ?? `candidate:${route.id}`}
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
  return (
    <>
      <PageBar.Root>
        <PageBar.Title>Applications</PageBar.Title>
        <PageBar.Action
          icon="compass"
          label="Refresh applications"
          placement="primary"
          onSelect={reload}
          disabled={discovery.loading}
        />
      </PageBar.Root>
      <Container size="4" px="5" pb="6">
        <ErrorNotice error={error} retry={refresh} />
        <Section size="1">
          <Flex direction="column" gap="4">
            <Flex align="center" gap="3">
              <Heading size="title-small">Connected</Heading>
              <Badge>{instances.length}</Badge>
            </Flex>
            {loading && !snapshot ? <Loading /> : null}
            {!loading && !error && instances.length === 0 ? (
              <EmptyState>
                <EmptyState.Illustration>
                  <Icon name="app-window" size="lg" />
                </EmptyState.Illustration>
                <EmptyState.Title>No connected applications</EmptyState.Title>
                <EmptyState.Description>
                  Connect a Flint Bridge from a running application to get
                  started.
                </EmptyState.Description>
              </EmptyState>
            ) : null}
            <Grid columns={{ initial: "1", sm: "2", lg: "3" }} gap="4">
              {instances.map((instance) => (
                <InstanceCard
                  key={instance.instance_id}
                  instance={instance}
                  revision={previewRevision}
                />
              ))}
            </Grid>
          </Flex>
        </Section>
        <Separator />
        <Section size="1">
          <Flex direction="column" gap="4">
            <Heading size="title-small">Available to connect</Heading>
            <ErrorNotice error={discovery.error} retry={discovery.reload} />
            {discovery.loading && !discovery.data ? (
              <Loading>Discovering applications</Loading>
            ) : null}
            {injectable.length ? (
              <Grid columns={{ initial: "1", sm: "2", lg: "3" }} gap="4">
                {injectable.map((candidate) => (
                  <CandidateCard key={candidate.pid} candidate={candidate} />
                ))}
              </Grid>
            ) : !discovery.loading && !discovery.error ? (
              <Text tone="muted">No applications available for injection.</Text>
            ) : null}
          </Flex>
        </Section>
        {manual.length ? (
          <Section size="1">
            <Flex direction="column" gap="4">
              <Heading size="title-small">Connect from host</Heading>
              <Grid columns={{ initial: "1", sm: "2", lg: "3" }} gap="4">
                {manual.map((candidate) => (
                  <CandidateCard key={candidate.pid} candidate={candidate} />
                ))}
              </Grid>
            </Flex>
          </Section>
        ) : null}
      </Container>
    </>
  );
}
