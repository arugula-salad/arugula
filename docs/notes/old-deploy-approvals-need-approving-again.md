---
section: Changed
title: Deploy approvals from before 0.66.6 must be given again
line: A deploy approved before 0.66.6, with a device's signature beside the passkey, is canceled instead of applied; approve it again.
headline: no
---
Since 0.66.6 a deploy approval is a passkey assertion alone (DK4). The older
form, which also carried a device's signature, stopped being made then, and
each one lived at most 7 days, so none is still in date. A deploy that still
waits on one is canceled when its apply is tried, saying the approval is
from before passkey-only approvals and to approve it again. A box refuses
one the same way. The old approvals still read and verify on the deploy's
page, for history.
