# MIDI trainer changes

Treat a reported bug as an example of a broader behavior. Before fixing it,
identify every affected exercise family and mode. Fix the shared policy or
component, and test across those cases. Use exercise-specific exceptions only
when the learning requirements justify them. Report exactly what the fix covers
and what remains unresolved.

For cross-cutting changes, define the intended behavior first, inspect all call
sites, and verify representative training and preview flows, correct and
incorrect answers, and compact and large terminals. Do not dispatch general UI
behavior by skill ID. Keep exercise-specific musical grading requirements
separate from shared presentation and session lifecycle policies.

Feedback stays in the exercise's established layout. Score comparisons put
Played above Expected. Failed or imperfect attempts remain available for review
until the user retries or continues. Successful, unassisted attempts may advance
automatically. Rhythm feedback must retain its detailed attack/hold comparison.

Run `bazel build //salsa/tools/midi_keys` and appropriate tests using the existing
Bazel configuration. Do not change build flags merely for convenience.

The header, device status, piano, insights, history, and global controls belong
in one shared outer training frame. Exercise renderers may only draw within the
exercise content area, including terminal images. Changing exercise families or
phases must not replace that outer frame. Any compact-terminal reductions must
be driven by terminal dimensions, consistently across exercise families.
