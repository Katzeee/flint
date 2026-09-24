import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { _electron } from "playwright-core";
import { preview } from "vite";

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));

test("the Cairn GUI shows backend state and preserves Flint actions", async () => {
  const server = await preview({ preview: { host: "127.0.0.1", port: 0 } });
  const address = server.httpServer.address();
  if (address === null || typeof address === "string") {
    throw new Error("Vite preview did not open a TCP port");
  }
  const application = await _electron.launch({ args: [join(appRoot, "tests/harness.cjs")], cwd: appRoot });
  try {
    const page = await application.firstWindow();
    await page.addInitScript(() => {
      window.__invokeCalls = [];
      window.__mockSnapshot = {
        backend: { ready: true, pid: 4312, registry_host: "127.0.0.1", registry_port: 9240 },
        instances: [],
      };
      window.__TAURI__ = {
        core: {
          invoke: async (command) => {
            window.__invokeCalls.push(command);
            if (command === "snapshot") return structuredClone(window.__mockSnapshot);
            if (command === "candidates") return { hosts: [{ host: "Blender", pid: 6120 }] };
            if (command === "stop_backend") return undefined;
            throw new Error(`Unexpected command: ${command}`);
          },
        },
      };
    });
    await page.goto(`http://127.0.0.1:${address.port}/`, { waitUntil: "load" });
    await page.getByText("No connected instances yet").waitFor();
    assert.equal(await page.getByText("Backend running").count(), 1);

    await page.evaluate(() => {
      window.__mockSnapshot.instances = [
        {
          instance_name: "Maya session",
          instance_type: "Maya",
          pid: 4520,
          runtime_version: "Python 3.11",
          execution_ready: true,
        },
      ];
    });
    await page.getByRole("heading", { name: "Maya session" }).waitFor({ timeout: 5000 });
    assert.equal(await page.getByText("1 connected").count(), 1);

    await page.getByRole("button", { name: "Scan applications" }).click();
    await page.getByText("Blender").waitFor();
    assert.equal(await page.getByText("PID 6120").count(), 1);
    if (process.env.FLINT_GUI_SCREENSHOT) {
      await page.screenshot({ fullPage: true, path: process.env.FLINT_GUI_SCREENSHOT });
    }
    await page.setViewportSize({ width: 500, height: 700 });
    await page.emulateMedia({ colorScheme: "dark" });
    const narrow = await page.evaluate(() => ({
      background: getComputedStyle(document.body).backgroundColor,
      overflows: document.documentElement.scrollWidth > window.innerWidth,
    }));
    assert.equal(narrow.overflows, false);
    assert.notEqual(narrow.background, "rgb(255, 255, 255)");

    await page.getByRole("link", { name: "Licenses" }).click();
    await page.getByRole("heading", { name: "Typography licenses" }).waitFor();
    await page.getByRole("link", { name: "Back to Flint" }).click();

    await page.getByRole("button", { name: "Stop backend" }).click();
    const dialog = page.getByRole("alertdialog", { name: "Stop Flint?" });
    await dialog.waitFor();
    await dialog.getByRole("button", { name: "Stop backend" }).click();
    assert.equal(await page.evaluate(() => window.__invokeCalls.includes("stop_backend")), true);
  } finally {
    await application.close();
    await new Promise((resolve, reject) => {
      server.httpServer.close((error) => (error === undefined ? resolve() : reject(error)));
    });
  }
});
