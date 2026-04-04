<!-- @format -->

# Privacy Policy

## This privacy policy is effective for Quadrant v25.3.2/26.02.2025 (26th of February 2025)+

## For previous versions of Quadrant's privacy policy look at the previous commits.

### The data that the Quadrant app uses.

- This app modifies your minecraft folder
- By default this app collects data on Windows. However, on GNU+Linux data is not being collected by default. You can let or prohibit the app to collect analytics and diagnostics from your PC by going to the settings and explicitly enabling or disabling optional data collection. Meanwhile my [website](https://mrquantumoff.dev) and my [API](https://api.usequadrant.dev) are proprietary, I can 100% guarantee that your personal info (e.g your hardware id or when did your device sent the diagnostics/analytics report) will not be sold to third parties.
- Why does the app need "hardwareId" and "date" fields? In order to not duplicate the same machine 100 times across the database and in order to remove super old data in the future.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    version: String,
    os: String,
    modrinth_usage: i32,
    curseforge_usage: i32,
    reference_file_usage: i32,
    manual_input_usage: i32,
    hardware_id: String,
    date: String,
    country: String,
}
```

This an example of which data is being collected by the app and can be accessed with https://api.usequadrant.dev/api/v3/quadrant/usage/get?hardware_id=YOURHARDWAREID.

However, when someone is asking for general usage info (https://api.usequadrant.dev/api/v3/quadrant/usage/get) without any query params, they can only get this struct

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfoAnonymous {
    version: String,
    os: String,
    modrinth_usage: i32,
    curseforge_usage: i32,
    reference_file_usage: i32,
    manual_input_usage: i32,
    country: String,
}
```

The Quadrant Share feature in Quadrant v12+ requires optional data collection enabled (hardware id), in order not to have the database spammed with hundreds of codes.

### The data that Quadrant ID accounts uses

#### Quadrant ID requires the user's email with the following purposes:

- Verifying that the account is not made by a bot
- Notifying the user of the changes/new sign-ins to their account

#### Quadrant ID requires the user's name to greet them when they opens their account details.

#### Quadrant ID requires the user to specify a password for authentication, it is securely hashed using argon2id and is used to delete the account, sign in to the account or to change the account data.

#### Quadrant ID requires the user to specify their username, it is used by other people, if they want to interact with the user (e.g Inviting the user to collaborate on Quadrant Sync).

### Usage of the Quadrant ID doesn't require turning on the optional data collection, but some of the features (such as Quadrant Share) do require it.
