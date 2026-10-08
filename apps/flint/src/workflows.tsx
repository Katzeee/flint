import {
  Badge,
  Box,
  Callout,
  Code,
  EmptyState,
  Flex,
  Heading,
  List,
  ListDetail,
  PageBar,
  Skeleton,
  Text,
} from "@cairn/ui";
import { RefreshCw } from "lucide-react";
import {
  readWorkflow,
  readWorkflows,
  type ConnectedInstance,
  type Execution,
  type Workflow,
} from "./backend.js";
import { executionPath, navigate, workflowPath } from "./navigation.js";
import { useResource } from "./resource.js";
import { Deferred, ErrorNotice, formatTime } from "./shared.js";

const placeholderNames = ["Export scene assets", "Rebuild lighting", "Publish animation"];

function PlaceholderRows({
  label,
  description,
  trailing,
}: Readonly<{ label: string; description: string; trailing?: string }>) {
  return (
    <Box aria-busy>
      <List.Root label={label}>
        {placeholderNames.map((name) => (
          <List.Item
            key={name}
            inert
            description={<Skeleton>{description}</Skeleton>}
            trailing={trailing ? <Skeleton>{trailing}</Skeleton> : undefined}
          >
            <Skeleton>{name}</Skeleton>
          </List.Item>
        ))}
      </List.Root>
    </Box>
  );
}

function statusTone(status: string) {
  return status === "succeeded"
    ? "success"
    : status === "failed"
      ? "danger"
      : "neutral";
}

function statusLabel(status: string) {
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function instanceName(instances: readonly ConnectedInstance[], id: string) {
  return instances.find((item) => item.instance_id === id)?.instance_name ?? id;
}

function executionName(execution: Execution) {
  return execution.name || `Execution ${execution.execution_id}`;
}

function OutputBlock({
  title,
  children,
}: Readonly<{ title: string; children: string }>) {
  return (
    <Flex direction="column" gap="2">
      <Heading as="h2" size="label" tone="muted">
        {title}
      </Heading>
      <Code aria-label={title} block>
        {children}
      </Code>
    </Flex>
  );
}

function ExecutionPage({
  workflow,
  execution,
  instances,
}: Readonly<{
  workflow: Workflow;
  execution: Execution | undefined;
  instances: readonly ConnectedInstance[];
}>) {
  const back = (
    <PageBar.Back
      label="Back to workflow"
      onSelect={() => navigate(workflowPath(workflow.workflow_id))}
    />
  );
  if (!execution) {
    return (
      <>
        <PageBar.Root>
          {back}
          <PageBar.Title>Execution</PageBar.Title>
        </PageBar.Root>
        <EmptyState>
          <EmptyState.Title>Execution not found</EmptyState.Title>
          <EmptyState.Description>
            This workflow has no execution with that identifier.
          </EmptyState.Description>
        </EmptyState>
      </>
    );
  }
  const status = execution.status;
  const errors = [execution.stderr, execution.traceback]
    .filter(Boolean)
    .join("\n");
  const silent = !execution.stdout && !errors && !execution.error;
  return (
    <>
      <PageBar.Root>
        {back}
        <PageBar.Title>{executionName(execution)}</PageBar.Title>
        <PageBar.Subtitle>{workflow.name || "Untitled workflow"}</PageBar.Subtitle>
      </PageBar.Root>
      <Box px="5" pt="2" pb="6">
        <Flex direction="column" gap="6">
          <List.Section title="Details">
            <List.Item
              trailing={
                <Badge tone={statusTone(status)}>{statusLabel(status)}</Badge>
              }
            >
              Status
            </List.Item>
            <List.Item trailing={instanceName(instances, execution.instance_id)}>
              Application
            </List.Item>
            <List.Item trailing={formatTime(execution.started_at)}>
              Started
            </List.Item>
            {execution.finished_at ? (
              <List.Item trailing={formatTime(execution.finished_at)}>
                Finished
              </List.Item>
            ) : null}
          </List.Section>
          {execution.error ? (
            <Callout.Root tone="danger">
              <Callout.Body>
                <Callout.Title>Execution failed</Callout.Title>
                <Callout.Text>{execution.error.message}</Callout.Text>
              </Callout.Body>
            </Callout.Root>
          ) : null}
          {execution.stdout ? (
            <OutputBlock title="Output">{execution.stdout}</OutputBlock>
          ) : null}
          {errors ? <OutputBlock title="Error output">{errors}</OutputBlock> : null}
          {silent ? (
            <Text tone="muted">
              {status === "running" || status === "pending"
                ? "Waiting for output…"
                : "No output recorded."}
            </Text>
          ) : null}
          <OutputBlock title="Code">{execution.code}</OutputBlock>
        </Flex>
      </Box>
    </>
  );
}

function WorkflowPage({
  workflow,
  instances,
}: Readonly<{
  workflow: Workflow;
  instances: readonly ConnectedInstance[];
}>) {
  return (
    <Flex direction="column" gap="6">
      {workflow.description ? (
        <Text as="p" tone="muted">
          {workflow.description}
        </Text>
      ) : null}
      <List.Section
        title="Executions"
        description={
          workflow.execs.length === 0 ? "No executions recorded." : undefined
        }
      >
        {workflow.execs.map((execution) => (
          <List.Item
            key={execution.execution_id}
            href={`#/${executionPath(workflow.workflow_id, execution.execution_id)}`}
            description={`${instanceName(instances, execution.instance_id)} · ${formatTime(execution.started_at)}`}
            trailing={
              <Badge tone={statusTone(execution.status)}>
                {statusLabel(execution.status)}
              </Badge>
            }
          >
            {executionName(execution)}
          </List.Item>
        ))}
      </List.Section>
    </Flex>
  );
}

export function Workflows({
  selectedId,
  executionId,
  instances,
}: Readonly<{
  selectedId?: string;
  executionId?: string;
  instances: readonly ConnectedInstance[];
}>) {
  const index = useResource("workflows", readWorkflows, 3000);
  const selected = useResource(
    selectedId ? `workflow:${selectedId}` : null,
    () => readWorkflow(selectedId!),
    1500,
  );
  return (
    <ListDetail.Root
      pane={selectedId ? "detail" : "list"}
      onPaneChange={(pane) => {
        if (pane === "list") navigate("workflows");
      }}
    >
      <ListDetail.List label="Workflows">
        <PageBar.Root>
          <PageBar.Title>Workflows</PageBar.Title>
          <PageBar.Action
            icon={RefreshCw}
            label="Refresh workflows"
            onSelect={index.reload}
          />
        </PageBar.Root>
        {index.error ? (
          <Box p="4">
            <ErrorNotice error={index.error} retry={index.reload} />
          </Box>
        ) : null}
        {index.loading && !index.data ? (
          <Deferred>
            <PlaceholderRows
              label="Loading workflows"
              description="12 executions · Yesterday"
            />
          </Deferred>
        ) : null}
        {index.data?.length === 0 ? (
          <EmptyState>
            <EmptyState.Title>No workflows yet</EmptyState.Title>
            <EmptyState.Description>
              Workflows created through Flint appear here with their execution
              records.
            </EmptyState.Description>
          </EmptyState>
        ) : null}
        <List.Root>
          {index.data?.map((item) => (
            <List.Item
              key={item.workflow_id}
              selected={selectedId === item.workflow_id}
              onClick={() => navigate(workflowPath(item.workflow_id))}
              description={`${item.execution_count} executions · ${formatTime(item.updated_at)}`}
              trailing={
                item.running_count > 0 ? (
                  <Badge>{item.running_count} running</Badge>
                ) : undefined
              }
            >
              {item.name || "Untitled workflow"}
            </List.Item>
          ))}
        </List.Root>
      </ListDetail.List>
      <ListDetail.Detail label="Workflow details">
        {executionId && selected.data ? (
          <ExecutionPage
            key={executionId}
            workflow={selected.data}
            execution={selected.data.execs.find(
              (item) => item.execution_id === executionId,
            )}
            instances={instances}
          />
        ) : (
          <>
            <PageBar.Root>
              <PageBar.Title>
                {selected.data?.name || (selectedId ? "Workflow" : "Workflows")}
              </PageBar.Title>
              {selectedId ? (
                <PageBar.Action
                  icon={RefreshCw}
                  label="Refresh execution records"
                  onSelect={selected.reload}
                />
              ) : null}
            </PageBar.Root>
            {!selectedId ? (
              <EmptyState>
                <EmptyState.Title>Select a workflow</EmptyState.Title>
                <EmptyState.Description>
                  Inspect its executions, code, output and errors.
                </EmptyState.Description>
              </EmptyState>
            ) : null}
            {selected.error ? (
              <Box p="5">
                <ErrorNotice error={selected.error} retry={selected.reload} />
              </Box>
            ) : null}
            {selected.loading && !selected.data ? (
              <Deferred>
                <Box px="5" pt="2">
                  <PlaceholderRows
                    label="Loading executions"
                    description="Maya session · 10:24"
                    trailing="succeeded"
                  />
                </Box>
              </Deferred>
            ) : null}
            {selected.data ? (
              <Box px="5" pt="2" pb="6">
                <WorkflowPage workflow={selected.data} instances={instances} />
              </Box>
            ) : null}
          </>
        )}
      </ListDetail.Detail>
    </ListDetail.Root>
  );
}
