import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { _electron } from "playwright-core";
import { preview } from "vite";
import { build } from "esbuild";

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));

// Exercise the generated bindings through Tauri's SDK transport mock.
const ipcMock = await build({
  stdin: {
    contents: `
      import { mockIPC } from "@tauri-apps/api/mocks";
      import { emit } from "@tauri-apps/api/event";
      mockIPC((command, args) => window.__mockInvoke(command, args), { shouldMockEvents: true });
      window.__emitEvent = emit;
    `,
    resolveDir: appRoot,
  },
  bundle: true,
  write: false,
  format: "iife",
});

async function withDesktop(run) {
  // Serve the window's own content security policy, so the view loads under the rules Tauri applies.
  const tauri = JSON.parse(await readFile(join(appRoot, "src-tauri/tauri.conf.json"), "utf8"));
  const server = await preview({
    preview: {
      host: "127.0.0.1",
      port: 0,
      headers: { "Content-Security-Policy": tauri.app.security.csp },
    },
  });
  let application;
  try {
    const address = server.httpServer.address();
    assert.ok(address && typeof address !== "string");
    application = await _electron.launch({
      args: [join(appRoot, "tests/harness.cjs")],
      cwd: appRoot,
    });
    const page = await application.firstWindow();
    await page.setViewportSize({ width: 1280, height: 850 });
    await page.addInitScript({ content: ipcMock.outputFiles[0].text });
    const cspViolations = [];
    await page.exposeFunction("__recordCspViolation", (violation) => {
      cspViolations.push(violation);
    });
    await page.addInitScript(() => {
      window.__cspReports = [];
      document.addEventListener("securitypolicyviolation", (event) =>
        window.__cspReports.push(
          window.__recordCspViolation({
            document: document.URL,
            directive: event.effectiveDirective,
            resource: event.blockedURI,
          }),
        ),
      );
    });
    await run(page, `http://127.0.0.1:${address.port}/`);
    await page.evaluate(() => Promise.all(window.__cspReports));
    assert.deepEqual(cspViolations, []);
  } finally {
    try {
      await application?.close();
    } finally {
      await new Promise((resolve, reject) => server.httpServer.close((error) => (error ? reject(error) : resolve())));
    }
  }
}

test("desktop navigation preserves connection identity and asynchronous selection", async () => {
  await withDesktop(async (page, url) => {
    await page.addInitScript(() => {
      window.__invokeCalls = [];
      window.__mayaApp = {
        instance_id: "maya-1",
        host: "maya",
        name: null,
        pid: 4520,
        local: true,
        connection: "not_connected",
        executable: "C:/Maya/maya.exe",
        runtime_version: null,
        bridge_version: null,
      };
      window.__mockSnapshot = {
        backend: {
          ready: true,
          pid: 4312,
          bridge_address: "127.0.0.1",
          bridge_port: 6321,
        },
        apps: [window.__mayaApp],
      };
      const summary = (id, name) => ({
        workflow_id: id,
        name,
        description: "",
        execution_count: 1,
        instance_ids: ["maya-1"],
        running_count: 0,
        failed_count: 0,
        updated_at: "2026-09-29T10:00:00Z",
      });
      window.__workflows = [summary("first", "Asset check"), summary("second", "Material check")];
      const record = (id, name) => ({
        workflow_id: id,
        name,
        description: "",
        created_at: "2026-09-29T10:00:00Z",
        execs: [
          {
            execution_id: "0001",
            name: "Inspect scene",
            instance_id: "maya-1",
            code: `print('${name}')`,
            status: "succeeded",
            stdout: name,
            stderr: "",
            error: null,
            traceback: null,
            started_at: "2026-09-29T10:00:00Z",
            finished_at: "2026-09-29T10:00:01Z",
          },
        ],
      });
      window.__mockInvoke = async (command, args) => {
        window.__invokeCalls.push({ command, args });
        if (command === "activate_title_bar") return "custom";
        if (command === "snapshot") {
          const current = structuredClone(window.__mockSnapshot);
          if (!window.__holdSnapshot) return current;
          return new Promise((resolve) => {
            window.__releaseSnapshot = () => resolve(current);
          });
        }
        if (command === "workflows") return structuredClone(window.__workflows);
        if (command === "workflow") {
          if (args.id === "first" && window.__delayFirst)
            return new Promise((resolve) => {
              window.__finishFirst = () => resolve(record("first", "Asset check"));
            });
          return record(args.id, args.id === "first" ? "Asset check" : "Material check");
        }
        if (command === "host_info") {
          if (args.preview !== true) throw new Error("Application cards must request previews explicitly");
          window.__captures = (window.__captures ?? 0) + 1;
          window.__peakCaptures = Math.max(window.__peakCaptures ?? 0, window.__captures);
          if (window.__captures > 2) {
            window.__captures -= 1;
            throw new Error("Window previews are busy");
          }
          await new Promise((resolve) => setTimeout(resolve, 80));
          window.__captures -= 1;
          if (args.pid === 4522)
            return {
              pid: args.pid,
              host: "maya",
              executable: "C:/Maya/maya.exe",
              window: { title: "Scene maya-3", minimized: true },
              preview: { unavailable_reason: "Window is minimized" },
            };
          return {
            pid: args.pid,
            host: "maya",
            executable: "C:/Maya/maya.exe",
            window: {
              minimized: false,
              title: args.pid === 4520 ? "Character_Rig.ma" : `Scene maya-${args.pid - 4519}`,
            },
            preview: {
              image:
                "data:image/svg+xml," +
                encodeURIComponent(
                  '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400"><rect width="640" height="400" fill="#344842"/><rect x="16" y="40" width="150" height="344" fill="#263630"/><rect x="182" y="40" width="442" height="344" fill="#78988b"/><text x="208" y="214" font-size="24" fill="white">Character_Rig.ma</text></svg>',
                ),
            },
          };
        }
        if (command === "focus_application") return;
        if (command === "attach") {
          const attached = {
            ...window.__mayaApp,
            name: "Maya session",
            connection: "ready",
            runtime_version: "Python 3.11",
            bridge_version: "0.1.0",
          };
          window.__mockSnapshot.apps = [attached];
          return attached;
        }
        if (command === "desktop_info")
          return {
            version: "0.1.0",
            state_dir: "C:/FlintData",
            control_endpoint: "127.0.0.1:6322",
            bridge_endpoint: "127.0.0.1:6321",
            attach_supported: true,
          };
        if (command === "start_backend") return window.__mockSnapshot.backend;
        throw new Error(`Unexpected command: ${command}`);
      };
    });
    await page.goto(url);
    await page.getByText("No connected applications", { exact: true }).waitFor();
    await page.getByRole("article").getByRole("link").click();
    await page.getByText("C:/Maya/maya.exe", { exact: true }).waitFor();

    // A status poll already in flight when attach succeeds still reports the process as not connected.
    // The attached application must not fall back to that answer, not even for one render.
    await page.evaluate(() => {
      window.__holdSnapshot = true;
    });
    await page.waitForFunction(() => typeof window.__releaseSnapshot === "function");
    await page.getByRole("button", { name: "Attach Bridge", exact: true }).last().click();
    await page.getByText("Ready to execute", { exact: true }).waitFor();
    assert.deepEqual(await page.evaluate(() => window.__invokeCalls.find((call) => call.command === "attach").args), {
      pid: 4520,
    });
    const pollsBeforeRelease = await page.evaluate(() => {
      window.__attachReappeared = false;
      new MutationObserver(() => {
        if (
          [...document.querySelectorAll("button")].some(
            (button) => button.textContent === "Attach Bridge" || button.ariaLabel === "Attach Bridge",
          )
        )
          window.__attachReappeared = true;
      }).observe(document.body, { childList: true, subtree: true, characterData: true });
      window.__holdSnapshot = false;
      window.__releaseSnapshot();
      return window.__invokeCalls.filter((call) => call.command === "snapshot").length;
    });
    await page.waitForFunction(
      (count) => window.__invokeCalls.filter((call) => call.command === "snapshot").length > count,
      pollsBeforeRelease,
    );
    assert.equal(await page.evaluate(() => window.__attachReappeared), false);

    // The same process must not remain in both sections, and its window operations retain its identity.
    await page.getByRole("button", { name: "Switch to application", exact: true }).click();
    await page.waitForFunction(() => {
      const image = document.querySelector('img[alt="Window preview of Character_Rig.ma"]');
      return image?.complete && image.naturalWidth === 640;
    });
    assert.ok(
      await page.evaluate(() =>
        window.__invokeCalls.some((call) => call.command === "focus_application" && call.args.pid === 4520),
      ),
    );
    await page.getByRole("button", { name: "Back to applications" }).click();
    await page.getByRole("link", { name: "Character_Rig.ma", exact: true }).waitFor();
    assert.equal(await page.getByRole("link", { name: "Maya", exact: true }).count(), 0);
    // The thumbnail belongs to the card's navigation target, not just its text link.
    const card = page.getByRole("article").filter({
      has: page.getByRole("link", { name: "Character_Rig.ma", exact: true }),
    });
    const thumbnail = card.locator("img");
    await thumbnail.waitFor({ state: "visible" });
    await thumbnail.scrollIntoViewIfNeeded();
    const imageBounds = await thumbnail.boundingBox();
    assert.ok(imageBounds);
    // The card's stretched link receives the pointer over the image.
    await page.mouse.click(imageBounds.x + imageBounds.width / 2, imageBounds.y + imageBounds.height / 2);
    await page.getByRole("button", { name: "Switch to application", exact: true }).waitFor();
    assert.equal(new URL(page.url()).hash, "#/apps/maya-1");

    // More cards than capture slots must all receive previews without flooding native workers.
    // A remote application sharing a local PID must not borrow that local window.
    await page.evaluate(() => {
      const app = window.__mockSnapshot.apps[0];
      window.__mockSnapshot.apps.push(
        { ...app, instance_id: "custom-2", host: "custom-editor", pid: 4521 },
        { ...app, instance_id: "maya-3", pid: 4522 },
        { ...app, instance_id: "remote-1", name: "Remote Maya", local: false, executable: null },
      );
    });
    await page.getByRole("button", { name: "Back to applications" }).click();
    await page.getByRole("link", { name: "Scene maya-2", exact: true }).waitFor();
    await page.getByRole("link", { name: "Scene maya-3", exact: true }).waitFor();
    const customCard = page.getByRole("article").filter({
      has: page.getByRole("link", { name: "Scene maya-2", exact: true }),
    });
    await customCard.locator("img").evaluate((image) => image.decode());
    assert.ok(await page.evaluate(() => window.__peakCaptures <= 2));
    await page
      .getByRole("article")
      .filter({
        has: page.getByRole("link", { name: "Scene maya-3", exact: true }),
      })
      .getByText("Window is minimized", { exact: true })
      .waitFor();
    await page
      .getByRole("article")
      .filter({ has: page.getByRole("link", { name: "Remote Maya", exact: true }) })
      .getByText("Running on another machine", { exact: true })
      .waitFor();

    await page.getByRole("link", { name: "Workflows", exact: true }).click();
    await page.evaluate(() => {
      window.__delayFirst = true;
    });
    await page.getByRole("button", { name: /Asset check/ }).click();
    await page.waitForFunction(() => typeof window.__finishFirst === "function");
    await page.getByRole("button", { name: /Material check/ }).click();
    await page.getByRole("heading", { name: "Material check", exact: true }).waitFor();
    await page.evaluate(() => {
      window.__finishFirst();
    });
    await page.getByRole("link", { name: /Inspect scene/ }).click();
    assert.equal(new URL(page.url()).hash, "#/workflows/second/executions/0001");
    assert.equal(await page.getByLabel("Output", { exact: true }).textContent(), "Material check");
    assert.equal(await page.getByLabel("Code", { exact: true }).textContent(), "print('Material check')");

    await page.setViewportSize({ width: 500, height: 750 });
    await page.getByRole("button", { name: "Back to workflow", exact: true }).click();
    await page.getByRole("link", { name: /Inspect scene/ }).waitFor();
    await page.getByRole("button", { name: "Back to Workflows", exact: true }).click();
    await page.getByRole("button", { name: /Material check/ }).waitFor();
    assert.equal(new URL(page.url()).hash, "#/workflows");
  });
});

test("backend controls recover from refusal and preserve pending work across navigation and reopening", async () => {
  await withDesktop(async (page, url) => {
    await page.addInitScript(() => {
      window.__invokeCalls = [];
      window.__startDuringStop = [];
      let stopping = false;
      const backend = {
        ready: true,
        pid: 4312,
        bridge_address: "127.0.0.1",
        bridge_port: 6321,
      };
      window.__mockSnapshot = { backend, apps: [] };
      window.__mockInvoke = async (command, args) => {
        window.__invokeCalls.push({ command, args });
        if (command === "activate_title_bar") return "custom";
        if (command === "snapshot") return structuredClone(window.__mockSnapshot);
        if (command === "candidates") return [];
        if (command === "desktop_info")
          return {
            version: "0.1.0",
            state_dir: "C:/FlintData",
            control_endpoint: "127.0.0.1:6322",
            bridge_endpoint: "127.0.0.1:6321",
            attach_supported: true,
          };
        if (command === "stop_backend") {
          if (!window.__allowStop) throw { code: "backend_busy", message: "executions are still active" };
          if (!window.__holdStop) {
            window.__mockSnapshot = { backend: null, apps: [] };
            return { stopped_pid: backend.pid };
          }
          stopping = true;
          return new Promise((resolve) => {
            window.__finishStop = () => {
              stopping = false;
              window.__mockSnapshot = { backend: null, apps: [] };
              resolve({ stopped_pid: backend.pid });
            };
          });
        }
        if (command === "start_backend") {
          window.__startDuringStop.push(stopping);
          window.__mockSnapshot = { backend, apps: [] };
          return backend;
        }
        throw new Error(`Unexpected command: ${command}`);
      };
    });
    await page.goto(`${url}#/settings`);

    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Stop backend", exact: true }).click();
    await page.getByText("executions are still active", { exact: true }).waitFor();
    await page.getByRole("alertdialog").waitFor({ state: "hidden" });
    await page.evaluate(() => {
      window.__allowStop = true;
    });
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Stop backend", exact: true }).click();
    assert.equal(
      await page.evaluate(() => window.__invokeCalls.filter((call) => call.command === "stop_backend").length),
      2,
    );
    await page.getByText("Not running", { exact: true }).waitFor();
    await page.getByRole("button", { name: "Start", exact: true }).click();
    await page.getByText("Running", { exact: true }).waitFor();
    await page.evaluate(() => {
      window.__holdStop = true;
    });
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await page.getByRole("alertdialog").getByRole("button", { name: "Stop backend", exact: true }).click();
    await page.waitForFunction(() => typeof window.__finishStop === "function");
    assert.equal(await page.getByRole("button", { name: "Stop", exact: true }).isDisabled(), true);
    await page.getByRole("link", { name: "Applications", exact: true }).click();
    await page.getByText("No applications running", { exact: true }).waitFor();
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    assert.equal(await page.getByRole("button", { name: "Stop", exact: true }).isDisabled(), true);
    // Reopening while stop is pending must establish the backend after that operation.
    await page.evaluate(() => window.__emitEvent("desktop-opened", null));
    await page.evaluate(() => window.__finishStop());
    await page.waitForFunction(
      () => window.__invokeCalls.filter((call) => call.command === "start_backend").length === 3,
    );
    assert.deepEqual(
      await page.evaluate(() => window.__startDuringStop),
      [false, false, false],
      "opening, starting, and reopening must not overlap a pending stop",
    );
    await page.getByRole("button", { name: "Stop", exact: true }).and(page.locator(":enabled")).waitFor();
    assert.equal(
      await page.evaluate(() => window.__invokeCalls.filter((call) => call.command === "stop_backend").length),
      3,
    );
    await page.getByRole("link", { name: "Applications", exact: true }).click();
    await page.getByText("No applications running", { exact: true }).waitFor();
  });
});
