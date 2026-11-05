# Bounded work packages and conditional estimates

Low/likely/high are expert engineering scenarios,not statistical P50/P90 or calibrated throughput. One engineer-day is eight focused hours including review/acceptance. Existing native/runtime/access components are reused;external waiting,human decisions and unrelated product work are excluded. Source/report publication is not product value.

| WP  | Outcome and acceptance gate                                                                               | Dependencies                             | Low/likely/high hours |
| --- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------- | --------------------- |
| W0  | Bind installed client/server/source/artifact and required consumer profiles                               | Existing owner receipts/metadata         | 8/12/20               |
| W1  | Resolve retained fork delta and qualify own install/agent identity without promoting unrelated ahead code | W0;existing allowlist candidate          | 8/16/32               |
| W2  | Real local/remote detach,reconnect,restore/resume and fault/effect safety receipt                         | W0/W1;native host profiles               | 16/32/56              |
| W3  | Authoritative manager/lane/task map and correct blocked/approval/session identity                         | W0;canonical registry and harness owners | 12/24/40              |
| W4  | Scoped read-only facade over supported server metadata API                                                | W0/W3;accepted data/verb allowlist       | 18/36/64              |
| W5  | Tunnel/Access deployment and anonymous/wrong-identity/revocation/origin-bypass negatives                  | W4;authorized access owner               | 16/32/56              |
| W6  | Independent native quality/denial/performance/recovery assurance                                          | W1/W2/W3;fixed profiles                  | 16/32/56              |
| W7  | Owned package/channel/update/rollback and evidence handoff                                                | W1/W2/W6                                 | 12/24/40              |
| M0  | Locate existing iOS source/artifact/owner receipt without broad rewrite                                   | Pose Snap gate tracked independently     | 2/4/8                 |
| M1  | Conditional integration/signing/verification of an already-working adopted iOS client                     | M0,W4/W5;Pose Snap signed+verified       | 20/40/80              |

CVP projection W0/W1/W2/W3/W6/W7 totals72/140/244 hours. MVP adds W4/W5:106/208/364 cumulative hours. These are planning envelopes for currently mapped work,not accepted delivery forecasts;existing passing evidence can remove work and newly discovered platform failures can exceed them. Required security/recovery behavior cannot be deferred merely to shrink CVP.

**Actual full maturity effort remains UNKNOWN because iOS identity/scope is unresolved.** If M0 recovers a working client compatible with the accepted facade,adding M0/M1 gives an illustrative128/252/452-hour envelope. It excludes designing a new mobile app,store/distribution waiting or architecture replacement. Do not represent this arithmetic as a known mobile completion total or divide it by nominal agent count.

First30-minute outcome:freeze product/source/installed identity and retain current metadata evidence. First1-hour outcome:bind manager/authority sources and identify one restore/denial witness. First4-hour outcome:qualify that witness or return its exact blocking artifact/profile packet. The24-hour point is a reassessment window,not a promised mobile/facade release. Calibrate elapsed forecasting only after real comparable accepted outcomes and owner/resource constraints exist.

Critical path W0->W1/W3->W2->W6->W7;facade W3->W4->W5 runs separately;mobile M0+Pose Snap gate precedes M1. Assign one writer per shared runtime/API file. Use isolated native state/servers and stronger-host compute for heavy gates;never restart shared services or send unapproved agent prompts during assessment.

Required receipts:source/binary hash,client/server/endpoint identity,host/session authority,correct fault continuation,denied unauthorized control,private-origin Access behavior and final package/rollback. Preserve failures and contradictory observations;API status success is not a substitute. Native iOS signing occurs only after Pose Snap local verification and its own artifact/profile identity are established.
