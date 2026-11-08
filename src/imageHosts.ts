/** @format */

// Icons and screenshots come from provider API data, which mod authors
// control. Loading them from any host would hand that host the user's IP
// address, so only the providers' own CDNs are fetched.
const IMAGE_HOSTS = new Set([
  "cdn.modrinth.com",
  "media.forgecdn.net",
  "edge.forgecdn.net",
  "mediafilez.forgecdn.net",
]);

/** The URL when it is an https image on a provider CDN, otherwise null. */
export function trustedImageUrl(url: string | null | undefined): string | null {
  if (!url) {
    return null;
  }
  try {
    const parsed = new URL(url.trim());
    return parsed.protocol === "https:" &&
      !parsed.username &&
      !parsed.password &&
      IMAGE_HOSTS.has(parsed.hostname)
      ? parsed.href
      : null;
  } catch {
    return null;
  }
}

/** Attributes every remote image carries: no referrer, loaded lazily. */
export const remoteImageProps = {
  referrerPolicy: "no-referrer",
  loading: "lazy",
  decoding: "async",
} as const;
