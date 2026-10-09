import { Callout, Flex, Spinner, Text } from "@cairn/ui";
import { useEffect, useState, type ReactNode } from "react";

export function ErrorNotice({ error, retry }: Readonly<{ error: string; retry?: () => void }>) {
  if (!error) return null;
  return (
    <Callout.Root tone="danger">
      <Callout.Body>
        <Callout.Title>Request failed</Callout.Title>
        <Callout.Text>{error}</Callout.Text>
      </Callout.Body>
      {retry ? (
        <Callout.Actions>
          <Callout.Action label="Try again" onSelect={retry} priority="primary" />
        </Callout.Actions>
      ) : null}
    </Callout.Root>
  );
}

// Loads that finish sooner than the delay show nothing, so a fast response never flashes a placeholder.
export function Deferred({ children, delay = 300 }: Readonly<{ children: ReactNode; delay?: number }>) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), delay);
    return () => clearTimeout(timer);
  }, [delay]);
  return shown ? children : null;
}

export function Loading({ children = "Loading" }: Readonly<{ children?: ReactNode }>) {
  return (
    <Flex align="center" gap="2" p="5" role="status">
      <Spinner size="sm" />
      <Text tone="muted">{children}</Text>
    </Flex>
  );
}

export function formatTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
