# Privacy policy

KWork is an open-source desktop client for DeepSeek Harness ACP. This policy describes the KWork desktop shell and its bundled runtime. Third-party services and plugins also have their own privacy policies.

## Local data

KWork stores workspaces, conversations, settings, scheduled tasks, and related runtime data on the user's computer. The Harness credential service stores credentials in `.credentials.yaml` under its home directory. KWork does not include a feature that publishes this local data as a public website.

## Network services

KWork connects to network services when the user installs or operates the corresponding feature. These connections include:

- DeepSeek account sign-in and account-related requests.
- Model discovery and model requests sent to DeepSeek or another provider selected by the user. Prompts, conversation context, and task-related content are sent to that provider as needed to perform the requested task.
- Web search, web fetching, plugin management, and other online features invoked or configured by the user.
- Scheduled tasks created by the user, which may contact the configured provider when they run.
- Downloads of required runtime components, such as Microsoft Edge WebView2 when it is missing.

The recipients and transmitted data depend on the providers, plugins, endpoints, and tasks selected by the user. Users should review the privacy policies of those third parties, including their chosen model provider and DeepSeek Harness services.

## Installation and removal

The Windows installer may download WebView2 if Windows does not already provide it. KWork can be removed through **Windows Settings → Apps → Installed apps → KWork → Uninstall**. Removing the application does not automatically delete user workspaces or Harness data.

## Contact

Questions about this policy can be reported through the [KWork issue tracker](https://github.com/kense-labs/kwork/issues).
