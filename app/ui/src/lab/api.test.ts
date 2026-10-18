import { describe, expect, it, vi } from "vitest";
import { LatestOnly } from "./api";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const settle = async () => { for (let i = 0; i < 4; i++) await Promise.resolve(); };

describe("latest-only channel ordering", () => {
  it("coalesces a reply callback's reentrant posts into the newest single call", async () => {
    const calls: string[] = [];
    const pending: ReturnType<typeof deferred<string>>[] = [];
    let channel!: LatestOnly<string, string>;
    channel = new LatestOnly((value) => {
      calls.push(value);
      const next = deferred<string>();
      pending.push(next);
      return next.promise;
    }, (_result, value) => {
      if (value === "A") {
        expect(channel.busy).toBe(true);
        channel.post("C");
        channel.post("D");
      }
    });
    channel.post("A");
    channel.post("B");
    pending[0].resolve("A");
    await settle();
    expect(calls).toEqual(["A", "D"]);
    expect(channel.busy).toBe(true);
    pending[1].resolve("D");
    await settle();
    expect(channel.busy).toBe(false);
  });

  it.each([false, true])("drops stale success/failure and cancelled pending work (reject=%s)", async (reject) => {
    const calls: string[] = [];
    const pending: ReturnType<typeof deferred<string>>[] = [];
    const onReply = vi.fn();
    const channel = new LatestOnly((value: string) => {
      calls.push(value);
      const next = deferred<string>();
      pending.push(next);
      return next.promise;
    }, onReply);
    channel.post("A");
    channel.post("B");
    channel.cancel();
    channel.post("C");
    if (reject) pending[0].reject(new Error("late failure"));
    else pending[0].resolve("late success");
    await settle();
    expect(onReply).not.toHaveBeenCalled();
    expect(calls).toEqual(["A", "C"]);
    pending[1].resolve("C");
    await settle();
    expect(onReply).toHaveBeenCalledExactlyOnceWith({ ok: true, value: "C" }, "C");
    expect(channel.busy).toBe(false);
  });

  it("honours cancellation inside the callback and a new post after that cancellation", async () => {
    const calls: string[] = [];
    const pending: ReturnType<typeof deferred<string>>[] = [];
    let channel!: LatestOnly<string, string>;
    channel = new LatestOnly((value) => {
      calls.push(value);
      const next = deferred<string>();
      pending.push(next);
      return next.promise;
    }, (_result, value) => {
      if (value === "A") {
        channel.post("C");
        channel.cancel();
        channel.post("D");
      }
    });
    channel.post("A");
    channel.post("B");
    pending[0].resolve("A");
    await settle();
    expect(calls).toEqual(["A", "D"]);
    pending[1].resolve("D");
    await settle();
    expect(channel.busy).toBe(false);
  });

  it("releases the gate and drains the pending request if a reply callback throws", () => {
    // Controlled IPC continuations let the test observe the thrown callback directly, without creating
    // an unrelated unhandled rejection in the runner.
    const calls: string[] = [];
    const replies: ((value: string) => void)[] = [];
    let channel!: LatestOnly<string, string>;
    channel = new LatestOnly((value) => {
      calls.push(value);
      return { then: (ok: (value: string) => void) => { replies.push(ok); } } as unknown as Promise<string>;
    }, (_result, value) => {
      if (value === "A") {
        channel.post("C");
        throw new Error("reply handler failed");
      }
    });
    channel.post("A");
    channel.post("B");
    expect(() => replies[0]("A")).toThrow("reply handler failed");
    expect(calls).toEqual(["A", "C"]);
    replies[1]("C");
    expect(channel.busy).toBe(false);
  });
});
