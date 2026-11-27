<!-- @format -->

# Privacy Policy

**Effective date:** 27 November 2026 (Quadrant 26.11.2 and later)

Earlier versions of this policy are available in the [commit history](https://github.com/QuadrantMC/quadrant/commits/next/PRIVACY_POLICY.md).

This policy explains what information Quadrant collects, why, how long it is kept, who it is shared with, and the choices you have. It covers:

- the **Quadrant** desktop app and **Quadrant-Native**, together with the `quadrantmc` command-line tool (the "app");
- **Quadrant ID** accounts and the account pages at [mrquantumoff.dev/account](https://mrquantumoff.dev/account);
- the Quadrant API at `api.usequadrant.dev`, including Quadrant Share, Quadrant Sync and settings sync;
- the website [usequadrant.dev](https://usequadrant.dev).

Quadrant is developed and operated by MrQuantumOFF ("we", "us"). For any privacy question or request, email [help@usequadrant.dev](mailto:help@usequadrant.dev).

**We do not sell your personal information, and we do not use it for advertising.**

## 1. Summary

- The app works without an account. Mods, modpacks and settings are stored on your computer.
- Basic **usage statistics** are on by default. You can turn them off in Settings at any time, and turning them off deletes them from our servers.
- A **Quadrant ID** account is optional. It stores your email address, username, display name and a hashed password, plus whatever you choose to sync or share.
- You can download all of your account data, or permanently delete your account, from the account page.

## 2. Usage statistics

### What is sent

When **Collect data** is turned on, the app sends the following to the Quadrant API:

- the app version;
- your operating system (Windows, macOS or Linux);
- the number of items downloaded from Modrinth and from CurseForge;
- an **installation ID**;
- a timestamp.

The installation ID is a random identifier (a UUID) created the first time the app runs. It is **not** derived from your hardware and is never synced to your other devices. Its purpose is to keep one record per installation instead of a new record on every launch. Usage statistics are not linked to your Quadrant ID account, but shares you create while signed in record the installation they came from (see section 3).

The server records the country your request came from, as reported by our network provider (Cloudflare). Your IP address is used for this lookup but is **not stored**. If no country is available, the server records "unknown".

### When it is sent

The app sends this data each time it starts, after you share a modpack with Quadrant Share, and when you turn the setting on. Nothing is sent while the setting is off.

### What is public

The usage figures are published at [usequadrant.dev/usage](https://usequadrant.dev/usage). The public view lists each installation's version, operating system, download counts and country, without installation IDs.

The record for a single installation can be viewed at `usequadrant.dev/usage/<installation ID>` by anyone who knows that ID. The app links to your own record in Settings. Treat your installation ID like a private link, and don't share it.

### Your choices

- Usage statistics are **on by default**. You can turn them off at any time in **Settings → Collect data**, or with `quadrantmc settings set collectUserData false`.
- Turning the setting off, or choosing **Delete your usage data**, deletes your installation's usage record and any anonymous Quadrant Share link it created.
- If you turn the setting off on another device and the change reaches this device through settings sync, your server record is not deleted. Use **Delete your usage data** on the device itself.
- Builds compiled without the default features (for example, some community packages) contain no usage reporting at all.

### Retention

Each installation has one record, which is overwritten every time new data is sent. The record is kept until you delete it as described above.

## 3. Quadrant Share

Quadrant Share creates a short code that other people can use to import your modpack.

- **What is stored:** the modpack manifest (its name, Minecraft version, mod loader and the list of mods with their download links), the share code, and how many times the code can still be used.
- **Who can see it:** anyone who has the code. They can view the modpack on usequadrant.dev or import it in the app.
- **What it requires:** Quadrant Share requires usage statistics to be on, because each share is tied to your installation ID. When you are signed in, the share is tied to both your account and your installation ID.
- **Retention:** a share is deleted once all of its uses are spent, when you create a new share (which replaces the old one), or when you delete your usage data (for anonymous shares) or your account (for account shares).

## 4. Quadrant ID

### Account information

To create an account, we collect:

- **an email address:** to verify that you control it, to reset your password, and to send security notices such as new sign-ins or email address changes;
- **a username:** public to other Quadrant users. Other people use it to invite you to a Quadrant Sync modpack;
- **a display name:** shown in your account and to the people you sync modpacks with;
- **a password:** stored only as an Argon2id hash. We never store or see your password in readable form.

### Security features

- **Passkeys:** we store your passkey's public key, the label you give it, and when it was created and last used. Your private key never leaves your device. We do not collect information about your device or authenticator.
- **Two-factor authentication (TOTP):** we store the secret your authenticator app uses to generate codes.
- **Verification codes:** codes we email you (for registration, password resets and email changes) expire after 5 to 15 minutes.

### Sign-ins

When you sign in, we determine the country of the sign-in so we can include it in security notices.

- On mrquantumoff.dev, the country comes from our hosting provider (Vercel).
- In other cases, it comes from our network provider (Cloudflare). If neither is available, your IP address is sent to [ipinfo.io](https://ipinfo.io) to look up the country.

Your IP address is never written to our database. It is held in server memory for rate limiting (up to one hour), and the country looked up for it is cached in memory for up to 30 days. Your session token contains your IP address, your country and a device name, and it expires after at most 72 hours.

If email notifications are turned off, security notices (the sign-in method and country) are saved as notifications in your account instead of being emailed.

### Cookies and local storage

The account pages keep your session token in your browser's local storage. The API may also set one strictly necessary cookie, `quadrant_id_token`, to keep you signed in. We do not use advertising or tracking cookies.

In the app, your sign-in tokens are stored in your operating system's secure credential store (Windows Credential Manager, macOS Keychain or the Linux Secret Service). They are removed and revoked when you sign out.

## 5. Quadrant Sync and settings sync

These features require a Quadrant ID.

- **Quadrant Sync** stores each synced modpack's name, Minecraft version, mod loader, mod list and the time it was last synced.
  - Every member of a modpack can see its contents and the usernames of the other members.
  - When someone changes the modpack, invites a member or removes one, the other members are notified with that person's display name.
- **Settings sync** is on by default when you are signed in. It stores a selected set of app preferences: data collection, enabled mod sources, news feeds, theme, interface scale, cache duration and similar options.
  - Only you can read it.
  - It never includes your installation ID, file paths or Minecraft folder location.
  - You can turn it off in Settings.

## 6. Signing in to other apps with Quadrant ID

Other applications can let you sign in with Quadrant ID. Before an app gets access, you are shown which permissions it is asking for. Depending on what you approve, the app may receive:

- your account ID;
- your display name and username;
- your email address;
- your Quadrant Sync modpacks, shares, synced settings and notifications.

Resetting your password revokes access for every app at once. To revoke a single app, sign out of it if it offers that option, or ask us to revoke it. Approved access tokens expire after one hour. Refresh tokens expire after 30 days and are then deleted. Once an app has received your data, its use of that data is governed by that app's own privacy policy.

## 7. Your rights and choices

From your account page you can:

- **view and edit** your username, display name, email address and notification preferences;
- **download your data:** a copy of your account, synced modpacks, synced settings, notifications, passkeys (without key material), app grants and shares;
- **delete your account:** this permanently deletes your account and everything linked to it, including passkeys, two-factor settings, notifications, app grants, synced settings, account shares, your Quadrant Sync memberships, and synced modpacks that would be left without an administrator. If you have an active subscription, cancel it before deleting your account.

Usage statistics are tied to an installation, not to an account. Deleting your account does not delete them; turn them off in the app as described in section 2. Notifications that other members already received about your activity in a shared modpack remain in their accounts.

Depending on where you live, you may have additional rights, such as the right to object to or restrict processing, or to lodge a complaint with your local data protection authority. To exercise any right that the account page doesn't cover, email [help@usequadrant.dev](mailto:help@usequadrant.dev).

## 8. Who we share data with

We share data only as described in this policy:

- **Email delivery:** our email provider receives your email address and the content of the messages we send you.
- **Cloudflare:** our API is served through Cloudflare, which processes requests to it (including your IP address) to deliver and protect the service.
- **ipinfo.io:** may receive the IP address of a sign-in to look up its country, as described in section 4.
- **Vercel:** hosts mrquantumoff.dev and runs Vercel Web Analytics and Speed Insights on it. These collect aggregated page-view and performance data without cookies.
- **Apps you authorize:** receive only the information you approve (see section 6).
- **Other Quadrant users:** can see your username, the display name used in shared modpacks, and the content of modpacks you sync with them or share codes for.
- **Quadrant administrators:** can view and manage account details (email address, username, display name, sign-in methods and group membership) to provide support and enforce the [Terms of Service](https://usequadrant.dev/terms). The administrator tools do not show the contents of your synced modpacks or settings.
- **Legal requirements:** we may disclose information if the law requires it.

## 9. Third-party services the app connects to

To do its job, the app connects directly to services we do not operate. Each one receives your IP address and the requests needed for the feature you are using, under its own privacy policy:

- **Modrinth** (`modrinth.com`): searching, browsing and downloading mods, and identifying mod files you already have. To identify files, the app sends their hashes, not the files themselves.
- **CurseForge** (`curseforge.com`, `forgecdn.net`): the same, using file fingerprints.
- **Prism Launcher metadata** (`meta.prismlauncher.org`): Minecraft and mod loader version information.
- **GitHub:** downloading app updates. The app asks the Quadrant API whether an update is available, sending your current version, channel, platform and architecture, and then downloads the installer.
- **blog.mrquantumoff.dev:** the in-app news feed.

The app does not use crash reporting, advertising or any other analytics services, and it does not connect to Mojang or Microsoft accounts.

## 10. Data stored on your device

The app stores its settings, your installation ID and a download cache in its data folder on your computer. Cached files are removed after 30 days by default; you can change this in Settings.

The app also creates and changes files in your Minecraft folder, which is how it installs mods, modpacks, resource packs and shaders. When you apply a modpack, the app replaces your `mods` folder with a link to that modpack, and backs up an existing `mods` folder first. Logs are printed to the console only. They are never saved to a file or uploaded.

## 11. Websites

- **usequadrant.dev** uses no analytics, no cookies and no third-party scripts. Fonts are served from our own server.
  - To offer the right installer, the download button checks your browser's reported operating system and architecture on your device.
  - Modpack pages load mod information and images from Modrinth and CurseForge.
- **mrquantumoff.dev** uses Vercel Web Analytics and Speed Insights, as described in section 8.

## 12. Server logs and security

The Quadrant API logs each request's method, path, response status and duration, for operating and debugging the service. **It does not log IP addresses or query strings.** IP addresses are held in memory for up to an hour to limit abuse, for example to cap repeated sign-in attempts. Our hosting and network providers (Cloudflare and Vercel) may keep their own request logs under their privacy policies.

All connections to our services use HTTPS. Passwords are hashed with Argon2id, and app secrets and refresh tokens are stored only as hashes. Sign-ins from some regions may be blocked for security or legal reasons.

No system is perfectly secure. If you find a vulnerability, please report it as described in our [security policy](https://github.com/QuadrantMC/quadrant/blob/next/SECURITY.md).

## 13. Retention

- **Account data, synced settings and notifications:** kept until you delete your account. Notifications can include the country of a past sign-in.
- **Synced modpacks:** kept until you delete them, or until you delete your account and no administrator is left.
- **Usage statistics:** one record per installation, kept until you turn data collection off or delete it.
- **Share codes:** kept until all uses are spent, the share is replaced, or the data it is linked to is deleted.
- **App authorization codes:** deleted after 10 minutes.
- **Access tokens:** expire after one hour.
- **Refresh tokens:** deleted after 30 days.
- **Emailed verification codes:** expire after 5 to 15 minutes.
- **IP addresses:** never written to our database. Held in memory for up to one hour for rate limiting; the country looked up for an IP is cached in memory for up to 30 days.

## 14. Children

Quadrant ID is not intended for children below the minimum age required to consent to online services in their country. If you believe a child has created an account without appropriate consent, contact us and we will delete it.

## 15. Changes to this policy

We may update this policy as Quadrant changes. The effective date at the top shows when it last changed. Every past version is available in the repository's [commit history](https://github.com/QuadrantMC/quadrant/commits/next/PRIVACY_POLICY.md). If a change significantly affects how your data is handled, we will announce it in the app or by email before it takes effect.

## 16. Contact

For questions, data requests or concerns, email [help@usequadrant.dev](mailto:help@usequadrant.dev).
