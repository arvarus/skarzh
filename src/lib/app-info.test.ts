import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { fetchAppInfo, formatAppInfo } from "./app-info";

describe("formatAppInfo", () => {
  it("renders the name and the version", () => {
    expect(formatAppInfo({ name: "skarzh", version: "0.1.0" })).toBe(
      "skarzh v0.1.0",
    );
  });
});

describe("fetchAppInfo", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("calls the app_info command and returns its payload", async () => {
    const info = { name: "skarzh", version: "0.1.0" };
    invokeMock.mockResolvedValue(info);

    await expect(fetchAppInfo()).resolves.toEqual(info);
    expect(invokeMock).toHaveBeenCalledWith("app_info");
  });

  it("propagates backend errors", async () => {
    invokeMock.mockRejectedValue(new Error("backend unavailable"));

    await expect(fetchAppInfo()).rejects.toThrow("backend unavailable");
  });
});
