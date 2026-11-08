/** @format */
import { describe, expect, it } from "vitest";
import { trustedImageUrl } from "./imageHosts";

describe("trustedImageUrl", () => {
  it.each([
    "https://cdn.modrinth.com/data/AANobbMI/icon.png",
    "https://media.forgecdn.net/avatars/1/2/3.png",
    "https://mediafilez.forgecdn.net/files/1/2/shot.png",
  ])("accepts %s", (url) => {
    expect(trustedImageUrl(url)).toBe(url);
  });

  it.each([
    "http://cdn.modrinth.com/icon.png",
    "https://tracker.example/pixel.gif",
    "https://cdn.modrinth.com.evil.example/icon.png",
    "https://evil.example/cdn.modrinth.com/icon.png",
    "https://user@evil.example@cdn.modrinth.com/",
    "javascript:alert(1)",
    "data:image/png;base64,AAAA",
    "not a url",
    "",
    null,
    undefined,
  ])("rejects %s", (url) => {
    expect(trustedImageUrl(url as string | null)).toBeNull();
  });
});
