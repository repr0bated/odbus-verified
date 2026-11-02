# Hackathon provenance — combined-main-hackathon merge

- Recorded: 2026-10-29T04:33:06Z
- Repo: https://github.com/repr0bated/odbus-verified.git
- hackathon tip: 2a6e40e9a40c334ca3d89581f551ea6e9233912a (tagged hackathon-premerge-20261029, backed up as branch hackathon-provenance-backup)
- main tip: 8435d5890ba5585c55b55ebb55828a7b66ba0ddf (tagged main-premerge-20261029)
- Merge-base: 268e80033a11c56c6ea616f28e0ee40b1754cae8
- Combined branch: combined-main-hackathon (merge of main into hackathon; hackathon branch itself NOT modified, rebased, or squashed)
- Recovery snapshot of pre-merge dirty checkout: refs/tbh/recovery/before-discard/20261029T043203Z-1102832 (build.rs, lib.rs, untracked catalog.rs on hackathon checkout)

## Hackathon-only commits (184) — full hashes, authors, dates

2a6e40e9a40c334ca3d89581f551ea6e9233912a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-29T04:13:02Z | hackathon: record move onto odbus-verified hackathon branch
f343af3b916aa37d1555af9cb69282665c9eca73 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-29T04:12:32Z | hackathon: exclude hackathon/ from workspace; AGENTS points work at this branch
4d815ca2b85220dd2f49cb3edce0eed77f6b3a0c | Jeremy Hobson <jeremy@3tched.com> | 2026-10-29T04:12:01Z | Merge etched-in-schema history into hackathon/ (provenance preserved)
c779bed2ec563852bac49a9219c895be27293dce | Jeremy Hobson <jeremy@3tched.com> | 2026-10-29T04:05:17Z | omp: deny local provider and meta->local fallback; record Meta 404 diagnosis
4d051ed148c66a9cbfd9e523e31094d758675815 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T23:48:51Z | specgen: provenance, orgchart, feed compositions + emitters; 43 tests
dfdaf3e933796680a00b5bb13c40e19ec369e253 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T23:33:54Z | fix: restore granted map dropped in struct-init edit
8097fdf2ed3e21c7cc53a4704d1eece6929d11c4 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T23:33:29Z | seal pin: level bound, catalog hash verify, verified grants gate; 11+40 tests
21e8d5753ca719e99567970d09de57de74032d22 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T23:23:44Z | oscal closure: sealed-operations props, table pinned to catalog; 39 tests
4ae2ce36eccfff66568a0c9eaf2518df946373b2 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T23:18:20Z | org_bridge: compile output into decide grants + GRANTS-READ-SPEC; 38 tests
8164c0974fe3cd91d07bd19f36fa3988262e8750 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T23:16:26Z | org-chart crate: sole grant authority, deep-gov + lone-key vectors; 9 tests
004cb9af24b2870f652ef7d77b31597b9508c9b5 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T22:56:55Z | specgen: grants console spec generator (validated spec) + example; 36 tests
ef424ac1554cb32398464ba345a1b1fd3d91d3ce | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T22:35:52Z | projection catalog (closed purposes, refuse-unknown) + CHAT-FEED-SPEC draft; 33 tests
7cc37b78a9982db1c12f2411a7c1af81ca3158bf | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T21:41:10Z | catalog-routing: sealed-truth capabilities, subset allow, unsealed fail-closed; 28 tests
d0d9093024b666bccd47dd450b6dc66a522d5835 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T20:24:17Z | catalog-routing crate: nested-catalog request routing (compliance/ui/models) + decide, 24 tests
3fcd5f725a10d54d08dc31f0dbbee4098d93bcfb | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:20:36Z | docs: the 3tched.com UI repo — pre-existing, references only, not hackathon code
b92e6517abe422fec6922af664a719e36c12e323 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:16:55Z | docs: dev cycle — each plugin is a schema feed; how state is read and archived
e52d9182a40100944ecf61a33cb912eea860bc24 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:15:12Z | docs: the UI is in the 3tchedFS dev cycle — derived from sealed blobs, actions call sealed methods
f9910c6eccf6ee503a83ff435b5d2b4ba02ce2bf | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:12:35Z | docs: make the 3tchedFS dev cycle the first thing every reader and agent sees
b28a3434bce1b53cf667233833d67d3e5929e4bc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T19:11:45Z | docs(wishlist): OD-124 remove MCP from the codebase after the hackathon
a565a4ddad42f2ae6dabb4544fb2de99b052d8d8 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T19:09:52Z | docs(agents): MCP retired — flagged as initial setup error; 3tchedFS is the interface
5e5bd0682e98aa4670f100438428156224047616 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:07:38Z | docs: no MCP — the identity acts only through its 3tchedFS mount
09155474f589e586c1ff5e8eb9cd92beb1bc653d | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:06:48Z | docs: dev cycle — blob, mount and identity relationship, highlighted in README and book
f61b9d41441e272c9f5d55dc1d16fd55f1e78ffa | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:03:42Z | docs: preload gRPC, protobuf, OVS, FUSE and memory skills; OpenFlow sources
1fb91e34ec1778a7f69f8cdb71daaa0ae52d18f6 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T19:02:57Z | ui: start from scratch — keep only a design-free element map and json-render docs; skill preload in AGENTS.md
8dfd8e87008f6a11ff958bfc9f44d991bd5430d4 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:52:55Z | chore: no-stubs policy — pre-commit check, clippy denies, Planned section in the book
339cfbf381578a4b6808e56e0badb924e2753dfd | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:48:01Z | docs: accuracy pass — owner + policy co-signer approvals, approvals crate status, Meta wired
ed51b4b43f3d25690163bd9bb64e989ea9e0441b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:45:54Z | docs: log published site
e1b3261ce499a140891b4971d1c9797da9649764 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:43:34Z | docs: mdbook guide + rustdoc, auto-published to Pages on push
5fae087ea3a80c234370775a24640d7ce65b39aa | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:38:48Z | approvals: pure-Rust two-approval root (types, hash, truth table, tickets; 12 tests)
8a8c8949e68c28d527bf3dd959c462cb7ca91825 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:36:52Z | chore: public-repo guard — pre-commit check for addresses, IDs and keys; push-often policy
84d4943c257ef559c18a5dfc66a0f338cf1ea012 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:35:28Z | docs: public-safe — placeholders for host addresses and identity IDs; real values local only
c42f80979beb56d0e2ef75835ebae53eb2aa1500 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:29:37Z | docs: log omp as the agent for this repo
16d8af1041d93c58c7835031f11e00f8767be091 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:29:29Z | docs: approvals spec; omp replaces Junie as the agent entry point
7747e81f33016b9e4b1f5ce67ddb1d717f7d93fa | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:26:44Z | docs: control-plane chatbot as policy co-signer for owner root actions
f40c5ca53a3be567767198f73f3c4423374ac187 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:25:24Z | docs: owner override of the two-approval rule as audited break-glass
79cba9be89f926df8e5e61914c137899ab7ffada | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:23:12Z | docs(network): etched0 was created out of band — record and follow-up
11415c951733c6c476d4b267c4e5573fc4918c87 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T18:20:33Z | docs(runtime): OpenFlow development bridge and grants
1b98e2c021e380f99555a5ca4a4ba13e7b556c52 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:20:31Z | feat(security): openflow.read for the Hackathon subsidiary; isolated etched0 bridge
b452e4747689c3c13810688fb60f7f0ba4188729 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:18:55Z | Policy: commit and push at end of every turn
a58f913192a971e663c0e77123e031f1fbac821e | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:18:39Z | Project setup: omp meta-only config, litellm helper, watchdog, trestle scaffolding
8f6e60ca07748dcbde68e7712068ee191d37e4fc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:13:00Z | trestle: workspace config for oscal/
733b7b47fd35ebdd069f835693c1248bab74982e | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T18:12:55Z | OSCAL foundation: 5-control catalog, tag taxonomy, portal profile, router data model
190ac9e7a87c1a91441b76bc18bcce4dbe0f0d31 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T17:40:05Z | docs(runtime): Meta Model API wired for development and the running service
7827e7c55e93460bf6f94914aa2aa0f3b74e11a6 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T17:23:00Z | junie: pin project model to muse-spark-1.3 (medium effort)
90f22301e9f061c47f99db9b97c71c95bfee0d34 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T17:10:33Z | docs(network): admin SSH reaches the host on 10.0.0.1:2222; DNAT table absent
e0eca62eefc3bedc497b2ed1b5868b06c2bab2a3 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T17:05:24Z | docs: runtime — develop as jeremy, run as 3tched, platform calls, supervision, network, models
561282ff445f6a85300de4a5e81911eac63a59b7 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T17:04:22Z | docs: START-HERE, Junie guidelines and bundled CONTEXT.md
a4d11704b758bbc9d83d8b80356c92088163e05b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T17:02:52Z | docs: generated backends — model writes definitions, fixed code runs them
306321807635980f804628663564b62ca644a1b3 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T17:00:49Z | docs: Matrix bot wording for the chat drop-in
f1164049aa09e31e70a7b75346068dacf7c5bfb3 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T17:00:39Z | docs: vendor-neutral team chat target (Matrix/Element first)
0f6e1773435b4a88babd716e3cacabda8319a55b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T16:57:50Z | docs: generator is proven — reuse it, build only the new surface and governance
4f18dd9a20298456b0d50183ab7d0c7381ef87cb | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T16:57:39Z | docs: drop-in spec — UI generator repurposed for Slack with governed actions
c758a551af99c4435678c77440a5d23e5ca0c910 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T16:56:16Z | feat: consumer/slack scaffold for the drop-in app
4993b5c2ff657df547dc1ebf799ef92ea20ee301 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T16:56:07Z | docs: Startup track scope — compliance routing, enforcement, policy, Slack drop-in
6858f9f4e2cbaaf9e1228646f5141d16116f6ce7 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T05:13:37Z | review: Jev strategy review — atomic judgments, decisions in code
88170f7595be59e79b7a5eec08d2343a29a1321e | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:55:33Z | docs: root holds authority without execution; two-approval rule for humans
9d688206923091a5555acb00d82b3dcca0740e8b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:53:59Z | docs: root is the only root on the system
8197df0996a110437d481cc53327bea2027cc6cc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:35:13Z | feat(deploy): runit unit for the Hackathon subsidiary 3tchedFS mount
693b6a92c10603edf1c475e77301ad6eadf4e5af | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:34:15Z | docs(network): hackathon identity internet-only egress; subsidiary mount
725f7889d0ff9d72e175e81528c0c813bb1eea9a | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T04:30:21Z | docs: ten priorities for winning the hackathon
81e8acf179a4e413717bba31f759e202241f84d1 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T04:26:06Z | docs: prior art, positioning and transport decisions for Spark
6bc0f709be4ef308fce8e7d85b95371b82256b5c | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:23:26Z | docs: record the remote-identity establishment design
6adcab35078d8c911903ccc38041ae8427b6e7c1 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T04:17:26Z | docs: README, 3tched resources, subsidiary/sled model, repo layout
1a4ab62ecd668070523438fee4868baa70f96bdb | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:11:41Z | docs: CMS controls-to-behaviour plan with evidence
ba78214f19923232c7334e62dc2eed3f7aa5823d | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T04:09:55Z | docs: drop identity projection from the plan; define the golden path
542b6df4e6d02fff6c5a0bee69193f9882b0cf32 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T04:06:31Z | docs: brief and agent instructions for Muse Spark
4d57691a7d74129a1ec7ce583d2d22470e3fbcaa | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T04:04:01Z | feat: Florida LLC declaration schema with OSCAL metadata header
a53b80a726316c1faf433b7d392e4d82086c5876 | Jeremy Hobson <jeremy.alan.hobson@gmail.com> | 2026-10-28T03:38:06Z | feat: index entry schema for the Etched in Schema Web3 table of contents
7646d6a926d4bbd8d57cda4ad38ff21b82257d87 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T03:19:03Z | docs(handoff): self-contained identity call proven; subsidiary model recorded
26519b9705889862c96093a5ad47e929723db8a3 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T02:45:46Z | fix(op-grpc-bridge): speak upstream zeroclaw.v1 to the router
adbb5b1b1815ce12f3973b55deac67f01fde448f | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T01:19:26Z | feat(deploy): chatbot-identity unit and chat-router generation grants
2839b22299bb87d66a722cb33632f3449ef209f1 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T00:24:56Z | docs(handoff): template uses only the shared 3tched-lungs profile
635d86117ade26daa4611a0fd4cb38182328e3bd | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T00:23:58Z | docs(handoff): owner confirms lungs Incus mount proven
62929c790daa3bc4e4179528c9d356b6e68ff6a2 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-28T00:22:34Z | docs(handoff): record release 0fd5bbf5 activation and boot-state repair
20b8dfa216dadcabe37bf7d5ad0858b1e33611d9 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-27T23:47:19Z | fix(deploy): run Salad control-plane helper as the router's root owner
71fa23240b09c75cacfe0cff536a5e62e08f03eb | Jeremy Hobson <jeremy@3tched.com> | 2026-10-27T18:43:53Z | chore(repo): checkpoint chat_router/EMQX refactor, chat_response fixes and chatbot adoption
ca58862ab7f1c14e97acbbd1471fe6ebfa8af00a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-23T23:11:55Z | docs(review): record PR repairs merges and remaining blockers
fa2147b3a17f0fce4dfa7098fab15de644ae03b2 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-23T21:27:11Z | fix(scripts): keep bearer tokens out of curl arguments
86f03d2dede738e2d780ab789dd5800b58678479 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-22T20:25:09-04:00 | docs(network): retain concurrent connection verification
4611c6ba3ff2c5c5f701d73e721936e4878b1526 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-22T20:23:14-04:00 | docs(repo): include concurrent session repair handoff
92152552116f8835095ba26e376087dfc7bc8b6d | Jeremy Hobson <jeremy@3tched.com> | 2026-10-22T20:21:26-04:00 | chore(repo): checkpoint consolidation, provisioning and audit work
051a20d056afba074e21d797aa34f8947c543c5a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-18T09:57:28Z | docs(repo): record checkpoint publication and local archive exclusions
7766577e7af741ea227ef1721167cd7dd3cc494f | Jeremy Hobson <jeremy@3tched.com> | 2026-10-18T09:55:01Z | chore(repo): checkpoint restoration and cognitive source rename
34ff9a502d953bb7b1520d530eed849b507e266a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T17:33:26Z | fix(router): bind Salad models to dedicated control plane profile
96a23f1596670d2f578b6d578217a05738f46cd0 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T14:18:48Z | fix(identity): restore chatbot storage before boot activation
fc8675a9f18ae0b59fc90f402d8e9bd5f15906e6 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T13:40:05Z | fix(incus): align pinned chatbot lifecycle name
f1364b81c2cc791e47a40129ce4a542cc8776f7d | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T13:15:13Z | fix(provisioning): resolve physical seed for cold remount
ac68e7a01508364463571b0736be29e89e00aeca | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T13:10:47Z | fix(provisioning): use physical seed and correct subvolume readonly flag
1f128d41c92a501675ce5fd6d65f08717765492b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T12:52:48Z | feat(provisioning): prepare pinned GPT chatbot adoption plan
343ebc4502f980dc24dfcfd7b7f49b0d7915baa3 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T12:49:28Z | fix(identity): prove the actual Incus mounted root
1686cd923fba08d95c553547ff8f0f0131dd2566 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T12:48:29Z | feat(identity): adopt partitioned chatbot root and initialize configured human
86786fd1cbd172c7e4562cbda853d26274137541 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T12:18:27Z | fix(provisioning): query incus root through supported json endpoint
e763b50e7da5010f8409c8966e4eb51c52990e66 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T12:03:41Z | feat(threetched): integrate sealed surfaces and identity provisioning work
d43211e78ab0b9a3387110ed86f8808cbcf454bc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T11:58:08Z | fix(provisioning): pair empty upgrade and layer it above seed
aaa5f32369c947e687c5192ca2843762e5278567 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T11:49:06Z | fix(provisioning): defer absent identities to explicit adoption
5a074720f5b7a35a7150c457fc0ca0b8df43cb0f | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T11:43:32Z | feat(provisioning): bind prepared root and configure guest fuse
9c4ea49a9b4f7cecaa5ea47d12f1ac0c19841d8b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T11:31:04Z | feat(provisioning): define stopped chatbot from prepared disk stage
9778c32f25d47913b6d207189c86a5fc424edfcd | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T09:03:24Z | feat(provisioning): pair upgrade snapshots and sync before chatbot boot
fd0504dd87e69f4c3299e3a5516ef2bb4f19349f | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T08:44:14Z | feat(provisioning): prepare partitioned seed leaf and upgrade filesystems
ab730c5100bc7d563ee357262e467205639f91fe | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T08:33:33Z | feat(provisioning): record partitioned instance disk preparation stage
2e0e7140dd2d4e60f28decd1bc2f5499e8dac5b2 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T07:04:40Z | fix(workspace-image): capture offline lungs declarations without live state
bde37cd1a42c5305c3b41ed14ae7a533ad204ceb | Jeremy Hobson <jeremy@3tched.com> | 2026-10-11T02:59:45Z | fix(network): rebuild OVS bridge seeding, scoped to the OD-104 regression
7beab346aa6c4b72443dac2d784af0bd623ea721 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-09T01:54:28Z | feat(op-plugins): add ZeroClaw skills and identity mount updates
ed2b4157377442363fd69b058d0395eb17b5e527 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-09T01:17:58Z | fix(deploy): stop packaging the on-host mail bundle in golden
e46dad8c40ab9f36235ce8765be9d89da67cf8f0 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T23:12:03Z | fix(op-cognitive-mcp): borrow chain link values from the timing block
3d94199f7a6ba325289ed86bb52cba7065677cdc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T23:08:15Z | fix(op-snowball): keep dump paths alive through the snapshot step
4ce8d019b881488875fb07170949524e371e0474 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T23:06:19Z | fix(op-cozo-store): copy chain-link strings into Cozo rows
f23dcb7b2326acf8871782e0408ada77b1c09c02 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T23:04:15Z | feat(op-snowball): dump timing and vectors into a new chain root
85c6880c8ffe2ec7425b781826de9f23d135a405 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T20:08:33Z | fix(security): grant control-plane chatbot router catalog and config caps
a1051cce1aad1e1d64134931d26e925a6cb5a230 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T20:06:27Z | fix(deploy): keep tun5 on warp-ssh path instead of ovsbr0
0258b2262a66f8399a4db6d9741c3332babb6414 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-08T19:07:13Z | feat(tched-router): extract model_routes from zeroclaw binary namespace
9e6bfd7c34ced12b3dc4af9693c79f77d3ddb316 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-07T10:29:24Z | docs(mcp): record deployed release and retained-source recovery
fbf98971c65b655848a3c992bb7b43c90152c24e | Jeremy Hobson <jeremy@3tched.com> | 2026-10-07T10:13:08Z | fix(mcp): preserve active fixes and admit scoped WireGuard origins
ed02a783fa9ae99c174544c585106e116ce2c6b1 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-05T22:56:23Z | docs: record the active f373feb release and the remaining filesystem artifacts
f373feb009bcebbde885a8e943731b510d62dfef | Jeremy Hobson <jeremy@3tched.com> | 2026-10-05T22:47:07Z | fix(tched-router): decode allowed roots when native config returns JSON text
4bb2b19596fe081554954f21e13f140e105649a2 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-05T22:28:23Z | chore(ui): pin the svelte dashboard that keeps refreshed models visible
a954da9621e2b2cbe6aeadbc7c645a002e887795 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-05T22:22:09Z | feat: release the filesystem mount and the svelte dashboard pin
26a4ad132fbbd4523b05724060bfdeb9ca661824 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-05T08:16:36Z | feat(identity)!: leaf-based clean provisioning and lungs ownership
db979516669dc3e2a73ad72eaf836aad3551270b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-05T04:43:33Z | refactor(tched-router)!: retire generated config surface; author the typed trio
3cf5c47d1dd3baec2522c5ddac03de5bba1c5523 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T18:46:48Z | fix(model-registry)!: bind catalog selections inside the provider's serve family
f6e07631e765df8b8cfcb50a0a6dd8f72bad7890 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T18:11:29Z | feat(model-registry)!: move the credential gate into the provider TOML
905772aa9368532b796ec8a7cee7af6cf2621475 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T17:27:24Z | docs(handoff): od-108 stage 2 landed and golden 20261004T172602Z prepared
77871e566b81dd1f0a12a72e26fd6bdc24c05d52 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T16:58:07Z | feat(tched-router)!: uniformize config surface to two authored typed methods
e29c07ec6459ea5eb593c9a42cc0454a7b4db065 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T13:00:40Z | feat(model-registry)!: native lookup, bash_models gate, $MODEL.n tokens, oauth-aware binder
67f241e192851e1df11c4a9fbc07d2a19f0e7fca | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T11:41:59Z | feat(ops): snapshot multi-agent working tree at 2026-10-04T11:5xZ
c7936fda979d6373b4d212b61128344946194f3d | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T08:03:52Z | fix(op-grpc-bridge,tched-router): propagate bounded provider error categories
c1e0fe69687bf2baa1aefd1f57e85a391c2a8d02 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-04T06:32:41Z | fix(op-cozo-store): read version-2 shared-human migration receipts
5536eda9aee40af360f3207f175c563049e99bc6 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:51:59Z | fix: complete validation integration - validate providers on every load
0950df7a279220192108509482c80aa7412bd0b2 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:50:12Z | feat: complete runtime error detection - validation + integration tests + SID1 audit
be9d1b00791252fb78cc9337e9a5a7589d0aca9d | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:49:03Z | feat: add model provider configuration validation at startup
0f7c7ec922d33b91ccb986d56cb4bc5973485564 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:44:12Z | fix: correct DataValue::Num pattern matching for count query
5c7e5a4e882b00f8b9a07cd6743ee6421347f5cf | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:43:12Z | fix(op-cozo-store): correct Cozo API usage in count_principal_conversations
030752c47f529fca6396d7b7ddfba8f01a942ef5 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:35:59Z | fix: resolve 20 code review findings - permissions, grants, state consistency, recovery
06de5b1e96452b74880979e92f35d0515ce0dd6a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T20:07:18Z | feat(op-grpc-bridge): catalog model Apply and stateless door chat routing
b2c4442bedad0e0e8d102332be0026bb087ad629 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T16:44:41Z | fix: resolve all clippy warnings and formatting issues
dd29aa624aea3badbfc55f64a4c092c969d19bee | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:43:09Z | script: interactive chatbot migration script (OD-98)
6038c5d4825e07c7d0af555e147c03718435021f | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:40:30Z | docs: OD-98 migration data gathering complete - ready for owner execution
21b264f48d8444271ac20655c76499124a4133b6 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:36:55Z | docs(handoff): OD-98 complete solution ready - chatbot migration ready for owner execution
8725a3ad5cb257e065e6004cfaace6a3f4936317 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:36:45Z | docs: update migration procedure with self-validation step
8c98e74405cbca0ab94e19caa8154224e4219d2a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:36:15Z | feat(op-cozo-store): add chatbot self-validation utility
51914795793cb90d683e975523c60566ca06ec01 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:34:31Z | docs(handoff): OD-98 migration ready - complete procedure documented
8cbdf58f570af1f90bfec9fb6744c3fb807f8974 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:34:23Z | docs: comprehensive OD-98 chatbot migration procedure guide
0ce71a80dc079e2c67f3d0a0253caeb9a31a386f | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:33:10Z | fix: remove unused imports from migrate-chatbot-root utility
24039f26f685f02eff7b0a61859e01db07345914 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:30:34Z | docs(handoff): OD-98 migration helper replaces simple update utility
118bd9067a2ec043a62ce4aa78316c497ab2e3fc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:29:43Z | feat(op-cozo-store): add proper migration helper for chatbot physical root
6aa243f7a7dabca21b4d71fdbaecc54ed10ae1fe | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:21:58Z | docs(handoff): session summary - OD-98 utilities ready, workspace clean
102ca36095a372c9def385d46b94c3e907e5e44b | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:21:24Z | fmt: workspace-wide import ordering and formatting
fdb6354da98504565919a704ad80f333452c9798 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:20:51Z | docs(handoff): OD-98 diagnostic utilities for FSID reconciliation ready for use
6006c601dc7caa2524eb7c4aa41515144c9bb0f5 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T20:20:05Z | feat(op-cozo-store): add diagnostic utilities for identity FSID reconciliation
d5f48548e3a55ada1276dc93e17f74358341c687 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T03:22:14Z | fix: governance_taxonomy type errors (String mismatches)
47dc269bca915f10f302ba625db584964a6d2c16 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-02T03:19:30Z | feat: add base governance taxonomy plugin (patient zero for chatbot)
ae33304aa829e46b3695f0ca9d211f8d0bd8484a | Jeremy Hobson <jeremy@3tched.com> | 2026-10-01T00:52:49Z | feat: record identity roots, the system graph, and the Go model catalog
46c8a19050cd5eb1bee8db7d62e303db141671df | Jeremy Hobson <jeremy@3tched.com> | 2026-09-30T10:37:46Z | build: pin verified identity workspace scaffold and final receipts
84a343b3da25a5dd884336888574c37b4f6e4dd1 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-30T10:34:39Z | fix: bound cache lifecycles and document routing foundations
abefbe435f91e4c719fabcea4e07383c5c146cfc | Jeremy Hobson <jeremy@3tched.com> | 2026-09-30T04:48:09Z | Expose every shell command through compact MCP.
9dd11564a13563e6c251dc2ec18aa22548c53242 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-30T01:31:21Z | Retire the local OpenCode server and let the chatbot schedule model refresh.
d1e5edcd51def88a2aa7480dfd4b54a636d277d9 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-30T01:07:08Z | feat(identity): provision soul subvolumes and list Anthropic and Salad
8e69e44c6eae1f3302d3d0a8dd851d32e07a9e17 | Claude <noreply@anthropic.com> | 2026-09-29T22:48:49Z | docs(handoff): compact MCP stays as the control plane's full-system access
6452219823c15187104116ee569bbd2dd704e5a9 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T22:03:59Z | feat(chat): carry chat_response onto this branch and keep model profiles out of the provider list
d57c49754bda8744fbee075a5d19617588a508a2 | Claude <noreply@anthropic.com> | 2026-09-29T08:30:26Z | feat(model-registry)!: provider TOML and a system-wide Cozo model catalog (OD-94)
5f30f47de87bb5a643aac291da601240ba16cf0b | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T08:09:39Z | docs(handoff): record pushes and stop unrequested compilation
70bbb5c36501c0e8a78069c8b4d8a299d07da4e3 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T08:01:30Z | chore(workspace): checkpoint pending plugin edits and provider investigation
5e203257b45278d6898f83b72495c676d3fd3168 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T07:01:54Z | feat(models-provider): add SpaceXai Grok with proper identity
e59f79958d3bbf0683c984530f7e1e95bce348c7 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T07:00:33Z | feat(models-provider): tie Claude identity to Anthropic provider
08a09488a67cc4fa56b3a9d2f3af245fc8c8f6a4 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T06:02:12Z | feat: wire ModelsProvider into gRPC service
5ff7769b5f2b3883e9beacf62004cba457fdd29b | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T06:00:05Z | feat: editable provider list with add/remove/update methods
bac9a6088e2cd7d614fbb87ba776da8c86387588 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T05:59:21Z | feat(cognitive-mcp): models provider registry
852bab0e7ca9965108be074091d1b682a8ca51d7 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T05:50:33Z | docs: semantic search architecture and configuration guide
533e40986369a4806bdb50e41c398bad97c76ccd | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T05:07:50Z | feat(semantic_search): wire to repos_repomix_voyage_context_4
6c66cb9fad0e78c2da8dd55e86ee671e7d858d01 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T04:42:45Z | feat(cognitive-mcp): add semantic_search service for Voyage+Qdrant
99fdd385f5748529fb30e8b0f57ab8e2ef436f65 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-29T03:33:03Z | refactor(cognitive-mcp): remove dead NotebookLM integration and repomix collection
03f7fbb8b3d88fd63f1edc949f6aad1462820bc1 | Claude <noreply@anthropic.com> | 2026-09-29T02:04:55Z | fix(op-grpc-bridge): clippy needless_borrow and wildcard_in_or_patterns
f97b74c836522c75ed96eb741e705c1f5e2d1d67 | Jeremy Hobson <jeremy@ghostbridge.tech> | 2026-09-28T19:13:07-04:00 | docs(handoff): toolsets vs the original HOT/WARM/EMQX design; owner moving to local
5f0b069fab20f005ce5025893a2333ec27283c06 | Claude <noreply@anthropic.com> | 2026-09-28T22:59:07Z | feat(threetched-fs,emqx): toolsets derived from the catalog, fed to EMQX
3cf28e46cc1529a223a82562a9b988dda12f31b9 | Claude <noreply@anthropic.com> | 2026-09-28T22:47:07Z | feat(threetched-fs): the identity's own MCP server over stdio
ce5761dd0a3b44df8e65540c5950918bb8e360f7 | Claude <noreply@anthropic.com> | 2026-09-28T22:30:46Z | feat(op-grpc-bridge)!: retire MongoDB MCP; one /mcp endpoint for any MCP client
058ce3413d3312e42d3e84ed0c4d176e7ab485d5 | Claude <noreply@anthropic.com> | 2026-09-28T21:59:06Z | feat(tched-router): add Cheaper Inference beside OpenCode Go; discard 268e800
b4d86446316c33e3636a05d82e66757fee4303f8 | Claude <noreply@anthropic.com> | 2026-09-28T18:30:19Z | feat(elevation)!: per-task sudo elevation for protected operations
02a65ce2ba5a8aa0ba25e275fa7aea7633bb17c4 | Claude <noreply@anthropic.com> | 2026-09-28T18:01:05Z | fix(tched-router): refuse an OpenCode Go key with spaces or control characters
29c9a10b9351c835d253ed6b3281babe3683ba52 | Claude <noreply@anthropic.com> | 2026-09-28T17:59:57Z | fix(tched-router): keep a door-set Control Plane model across restarts
ee1fcf5c28c3c97497120d6e61224c8a8af430f7 | Claude <noreply@anthropic.com> | 2026-09-28T17:57:00Z | docs(handoff): record the picker removal, OpenCode Go and OD-93
5df142897c25d861b2d33d3f7d06ed0b3f86d1dd | Claude <noreply@anthropic.com> | 2026-09-28T17:40:23Z | feat(json_render)!: UI gallery candidates and an active catalog with promotion (OD-93)
5a684d5655744433ec1a8e2277a04e75a3eb7f5c | Claude <noreply@anthropic.com> | 2026-09-28T17:13:25Z | feat(tched-router): route OpenCode through OpenCode Go as a native provider
9534bbb15fd784a691f534e61208470427280790 | Claude <noreply@anthropic.com> | 2026-09-28T17:10:21Z | revert(op-web): remove the sudo-password model picker and the Zen provider switch

## Main-only commits (11)

8435d5890ba5585c55b55ebb55828a7b66ba0ddf | Jeremy Hobson <jeremy@ghostbridge.tech> | 2026-10-23T17:02:33-04:00 | Merge pull request #7 from repr0bated/cursor/crd-cursor-keepass-5bbc
6595943ae46bd0f598f9720e6eeb2ed19d433f72 | Jeremy Hobson <jeremy@3tched.com> | 2026-10-23T21:01:49Z | fix(crd): prepare Cursor key storage as the desktop user
02c7ce85ff093026c1165237b5caf056bfb4dcfc | Jeremy Hobson <jeremy@3tched.com> | 2026-10-03T13:55:10-04:00 | Merge pull request #5 from repr0bated/feat/chat-response
6fa6e082f59b76551cc7578207c828882fad1822 | Cursor Agent <cursoragent@cursor.com> | 2026-10-01T02:10:51Z | docs(handoff): record the KeePassXC Cursor draft PR
2a9e0a7f83d3c42b4bb20cf31eeb6ebecad551f0 | Cursor Agent <cursoragent@cursor.com> | 2026-10-01T02:10:26Z | fix(crd): store Cursor secrets in KeePassXC on Plasma
cac5cfb360d94f5593edb372256ee06753d70923 | Jeremy Hobson <jeremy@3tched.com> | 2026-09-28T23:16:21Z | Merge origin/main into feat/chat-response
2d2679eb749b3c5e71da07934cfa66054a7a603c | Jeremy Hobson <jeremy@3tched.com> | 2026-09-28T19:01:38Z | docs(handoff): host session continues OD-90 from the cloud
e4877bfab99d3f88c50ce10fce03c2b20df4a079 | Claude <noreply@anthropic.com> | 2026-09-28T15:22:04Z | Merge remote-tracking branch 'origin/main' into feat/chat-response
fa6be9a478622b98a9871b4154265076de327423 | Claude <noreply@anthropic.com> | 2026-09-28T12:42:07Z | fix(op-grpc-bridge): announce every durable chat turn and ring the asker on recovery
1d71d007391bd899098e33f67a41aa27ce4bf9b9 | Claude <noreply@anthropic.com> | 2026-09-28T12:24:23Z | feat(chat_response): record both sides of a conversation
cb81f013cdf79deffeeb9c2944b2a12b5f3e83ca | Jeremy Hobson <jeremy@3tched.com> | 2026-09-28T11:29:13Z | feat(chat_response): OD-90 stage 1 plugin, Cozo store, and bridge dispatch

## crates/op-grpc-bridge presence (committed trees)
Cargo.toml
SPEC.md
build.rs
compare-op-grpc-bridge.md
examples
...
## Hackathon diffstat vs merge-base (summary)
 vendor/tched-router/test-build-native.sh           |     76 -
 vendor/tched-router/test-build-threetched.sh       |     56 +
 4973 files changed, 994408 insertions(+), 53318 deletions(-)
