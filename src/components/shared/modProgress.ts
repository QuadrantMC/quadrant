/** @format */
import { listen } from "../../desktop";

type ProgressListener = (progress: number) => void;

interface Channel {
  subscribers: Map<string, Set<ProgressListener>>;
  count: number;
  unlisten: Promise<(() => void | Promise<void>) | null> | null;
}

/**
 * One Tauri listener per progress event, however many cards are on screen. A
 * pack with hundreds of mods used to register two listeners per card and
 * deliver every progress tick to all of them.
 */
function createChannel(event: string) {
  const channel: Channel = { subscribers: new Map(), count: 0, unlisten: null };

  function start() {
    channel.unlisten = listen(event, (e: any) => {
      const listeners = channel.subscribers.get(e.payload?.modId);
      if (listeners) {
        for (const fn of [...listeners]) {
          fn(e.payload.progress);
        }
      }
    }).catch((error) => {
      console.error(error);
      return null;
    });
  }

  return function subscribe(modId: string, fn: ProgressListener) {
    let listeners = channel.subscribers.get(modId);
    if (!listeners) {
      listeners = new Set();
      channel.subscribers.set(modId, listeners);
    }
    listeners.add(fn);
    if (channel.count++ === 0) {
      start();
    }
    return () => {
      listeners.delete(fn);
      if (listeners.size === 0 && channel.subscribers.get(modId) === listeners) {
        channel.subscribers.delete(modId);
      }
      if (--channel.count === 0) {
        const pending = channel.unlisten;
        channel.unlisten = null;
        void pending?.then((unlisten) => unlisten?.());
      }
    };
  };
}

export const subscribeModDownloadProgress = createChannel("modDownloadProgress");
export const subscribeModInstallProgress = createChannel("modInstallProgress");
