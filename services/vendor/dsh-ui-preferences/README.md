# Harness client preference Host modules

Bundled without behavioral changes from deepseek-harness commit
39163dd30f703a79079e2d2446545cff3c453738, packages/client/ui-theme and
packages/client/ui-settings. MIT license is retained.

Only the Schemastery import is external. At launch it resolves to the running
Host's schema implementation, as required by Cordis volatile configuration.
KWork adds missing modules as a profile bundle, below user configuration.
Existing namespace owners, explicit disabled entries and profile patches are
preserved. The Host Settings service owns revisions and persistence.

On macOS the same bundle restores the base profile's official HMR service,
which ACP disables by default. Module roots remain empty; explicit profile
files use upstream watchers. A user's explicit HMR opt-out still takes priority.
