#!/usr/bin/env bash
# 3tched skills installer. Catalog verified 2026-10-12.
# Usage: bash install_3tched_skills.sh --project /path/to/repo
# Preview: bash install_3tched_skills.sh --dry-run
# List: bash install_3tched_skills.sh --list
# Requires Bash, Python >= 3.9, Git, and HTTPS access to GitHub. No sudo needed.
set -euo pipefail
command -v python3 >/dev/null 2>&1 || { echo 'Python 3 is required.' >&2; exit 1; }
exec python3 - "$@" <<'PYTHON_3TCHED_INSTALLER'
import argparse
import collections
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

# Verified upstream skill directories, pinned commits, and SKILL.md checksums.
# This installs instructions and their supporting files, not application packages.
MANIFEST = [{'group': 'generative-ui', 'name': 'core', 'repo': 'vercel-labs/json-render', 'path': 'skills/core', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': 'd0e8a8a3c419046952d4652240a9ec50bd0349ed673f1e44ad5fbb29d370a452'}, {'group': 'generative-ui', 'name': 'svelte', 'repo': 'vercel-labs/json-render', 'path': 'skills/svelte', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': '03e6e301b51996001cc1641ee8ace97209a49fe948d2f1c8d3304f2b620e584e'}, {'group': 'generative-ui', 'name': 'shadcn-svelte', 'repo': 'vercel-labs/json-render', 'path': 'skills/shadcn-svelte', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': '2afe2d38f582163d99aa280b0ef7504091a29c3cfa42dba663613cd746d16e47'}, {'group': 'generative-ui', 'name': 'devtools', 'repo': 'vercel-labs/json-render', 'path': 'skills/devtools', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': '3dacf170385b7878fea7d3a3c32b4650b959962810278df8390cd2b1cc150d0b'}, {'group': 'generative-ui', 'name': 'devtools-svelte', 'repo': 'vercel-labs/json-render', 'path': 'skills/devtools-svelte', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': '99b85675f5c08b0ddc147a29306fee1b7a713d1cddb7e22dd8b1ebd8ba074b03'}, {'group': 'generative-ui', 'name': 'directives', 'repo': 'vercel-labs/json-render', 'path': 'skills/directives', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': 'af32f33e68b73393306d759b52b9e4f753ca9502e2411110df46f8350c76969c'}, {'group': 'generative-ui', 'name': 'yaml', 'repo': 'vercel-labs/json-render', 'path': 'skills/yaml', 'commit': 'fc2a696a50a30cb30c878ab1eb65e102487eea0f', 'sha256': '510a4898a18df93d6cd0b6dbb28cbd23c4a06a25dae815c6aa5d4ea6999370f5'}, {'group': 'generative-ui', 'name': 'ai-sdk', 'repo': 'vercel/ai', 'path': 'skills/use-ai-sdk', 'commit': 'ba05943b69ad070558bb5bc912da99ec66d7d263', 'sha256': '6bc642319c4ecbf9fbd59cef783b02174d4bd663c010703a6a6b80bda855b32d'}, {'group': 'coordination', 'name': 'dispatching-parallel-agents', 'repo': 'obra/superpowers', 'path': 'skills/dispatching-parallel-agents', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '1968923066f3b707eb01d1992cdf4c42284c3855f70253b9cd5000ff45fca13c'}, {'group': 'coordination', 'name': 'subagent-driven-development', 'repo': 'obra/superpowers', 'path': 'skills/subagent-driven-development', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '8dde5589ee083fb4999106d116f01c8fdaf3116a8a9813fdedeb80329de49046'}, {'group': 'coordination', 'name': 'using-git-worktrees', 'repo': 'obra/superpowers', 'path': 'skills/using-git-worktrees', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '8cfb86f121269e8f7f12361e6795c4f6738828340e28964c9229d365666c9edd'}, {'group': 'coordination', 'name': 'writing-plans', 'repo': 'obra/superpowers', 'path': 'skills/writing-plans', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': 'a6c67c1900064347c2a329990dd3c555657c51c3ec53b259a08aa01a2c26139a'}, {'group': 'coordination', 'name': 'requesting-code-review', 'repo': 'obra/superpowers', 'path': 'skills/requesting-code-review', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '2180e14a448e9410bec88e6646a2d1f0ce7fdd58de27388c1e052e929679d2bc'}, {'group': 'coordination', 'name': 'receiving-code-review', 'repo': 'obra/superpowers', 'path': 'skills/receiving-code-review', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '091df1629510af1b92fc4abd6f96732ebedb4cb2c0f3457e8f2740b0504a2438'}, {'group': 'coordination', 'name': 'verification-before-completion', 'repo': 'obra/superpowers', 'path': 'skills/verification-before-completion', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '2befe7fc55bcadaa3d97dd9e8efeb633d2561c0ebe74c5a8b17c4d9e7e4520b3'}, {'group': 'coordination', 'name': 'systematic-debugging', 'repo': 'obra/superpowers', 'path': 'skills/systematic-debugging', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '51c6435d98c83c42c96ba7c35725a0b2c376cd487fb1a4e6bd0ddbe7e33ef202'}, {'group': 'coordination', 'name': 'test-driven-development', 'repo': 'obra/superpowers', 'path': 'skills/test-driven-development', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '64b03fce4aee5a97a93160cea8111f3ba13a17b7c001db4bd5836d67fd10705d'}, {'group': 'coordination', 'name': 'finishing-a-development-branch', 'repo': 'obra/superpowers', 'path': 'skills/finishing-a-development-branch', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '8db5a922b242dd4e1bf824cb91c13b3e8d8e8a86d6ceaf7f0774eb9cce909d65'}, {'group': 'coordination', 'name': 'using-superpowers', 'repo': 'obra/superpowers', 'path': 'skills/using-superpowers', 'commit': 'bb92a77741419a4ab5f06e711a283343f1ada0c3', 'sha256': '82c5c8866ad7f5dd4440ce66bd7806ba48a2f13771beae5cf112e53f08fe36ba'}, {'group': 'coordination', 'name': 'agent-patterns', 'repo': 'oakoss/agent-skills', 'path': 'skills/agent-patterns', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '892756ffd9a16b6518e2048cc053b93908140a4accdd1c91184746db7078b04b'}, {'group': 'coordination', 'name': 'orchestration', 'repo': 'oakoss/agent-skills', 'path': 'skills/orchestration', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '1688f468b746e8d7a7b242749885b529afda129727584ac9d7646f8b893e3e06'}, {'group': 'coordination', 'name': 'beads-workflow', 'repo': 'oakoss/agent-skills', 'path': 'skills/beads-workflow', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': 'b5dba23dab108bacfa3c5a137f6b4398dffae49a2cec2f75d1da699c4219273c'}, {'group': 'coordination', 'name': 'beads-viewer', 'repo': 'oakoss/agent-skills', 'path': 'skills/beads-viewer', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '031f5bbfff150c0655a9a6bf172a79795d09e075d3f24ab189aafeb7f9973beb'}, {'group': 'generative-ui', 'name': 'design-system', 'repo': 'oakoss/agent-skills', 'path': 'skills/design-system', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '70d7fd95d699b06711fb69410f58a19ad9c06292dbbd2afb3ac9c8c554f6ca39'}, {'group': 'generative-ui', 'name': 'typescript-patterns', 'repo': 'oakoss/agent-skills', 'path': 'skills/typescript-patterns', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '284ebc8268e66caeb04f2c83dea810ac782fecb31653a4bd7659802b81529d13'}, {'group': 'generative-ui', 'name': 'zod-validation', 'repo': 'oakoss/agent-skills', 'path': 'skills/zod-validation', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': 'b29a41445211979786bcbb3dff5f707ba00f27ecea3d9d93103fb87f32c1def5'}, {'group': 'protocols', 'name': 'openapi', 'repo': 'oakoss/agent-skills', 'path': 'skills/openapi', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': 'b481eb0b9f6895f6e4124a507eb3ff484c2dd57f1175f543f69ae866b7c40e5d'}, {'group': 'generative-ui', 'name': 'accessibility', 'repo': 'oakoss/agent-skills', 'path': 'skills/accessibility', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '015cae5da11a9056f3624973d9cced284b5b0c2beba40dc1e6c2e776eeaa1f65'}, {'group': 'verification', 'name': 'playwright', 'repo': 'oakoss/agent-skills', 'path': 'skills/playwright', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '9702fa49f1f705a660d54f092f5340504c2b15eec93db65d8ab57d9b9b65bad1'}, {'group': 'generative-ui', 'name': 'realtime-sync', 'repo': 'oakoss/agent-skills', 'path': 'skills/realtime-sync', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '6691cd2d290f85946d803d7e045ebb3744f0885587f3a9be145b9c15b4a9b5b0'}, {'group': 'memory', 'name': 'knowledge-graph-builder', 'repo': 'oakoss/agent-skills', 'path': 'skills/knowledge-graph-builder', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '2e96af925fd6b536eac9094c979ae3854dc738369ede82996d0931e7a39af3d3'}, {'group': 'protocols', 'name': 'mcp-expert', 'repo': 'oakoss/agent-skills', 'path': 'skills/mcp-expert', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '83c0c98b9b103b754769092b72e389b76dc8eca5ba5e4f6fe69a1d46d2c9ea3b'}, {'group': 'verification', 'name': 'application-security', 'repo': 'oakoss/agent-skills', 'path': 'skills/application-security', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': '869c472c581e2d6859c48178deef72f09dd9a7960c781552595a0f3059f2cd48'}, {'group': 'verification', 'name': 'quality-auditor', 'repo': 'oakoss/agent-skills', 'path': 'skills/quality-auditor', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': 'b559cae198e96f3a014d6ac9a7bb694bef46bf26e09764fa6925f9c449b29087'}, {'group': 'storage', 'name': 'rust', 'repo': 'oakoss/agent-skills', 'path': 'skills/rust', 'commit': '85e3a3919d9e0ec7f7302a5143ec4b3e66f5f6ad', 'sha256': 'bd166091b2bba7ddd11e25397c3071b100de60d66b3a290ef9756d9f13822f57'}, {'group': 'protocols', 'name': 'protobuf', 'repo': 'bufbuild/claude-plugins', 'path': 'plugins/protobuf/skills/protobuf', 'commit': 'c4766e5fff1b52283f38bd61122103efe58c60fd', 'sha256': '8b09f409f34a024a37c582fdbe81fb12740ea8a9d446becc8f8c7550d57e11ee'}, {'group': 'compliance', 'name': 'trestle-assessment', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-assessment', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '4ca36880f8e7e7cfe8ce68f426568a71e0a25af3903f2ac0c0047aab54a0c080'}, {'group': 'compliance', 'name': 'trestle-authoring-workflow', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-authoring-workflow', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '607cfb4a19e89c392568fae995bb3a29ce87f32d7d52f3092e7c3512a1fc193a'}, {'group': 'compliance', 'name': 'trestle-compliance-pipeline', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-compliance-pipeline', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': 'aabc5c42ee24b78d42af9060c121e2af6203fcdd0b523b43eb3643ff3d1a1aa3'}, {'group': 'compliance', 'name': 'trestle-control-implementation', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-control-implementation', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '1926c62e7b218717ebf3f4375aa13a12c4cc1661ff15f19bc6026b947809da32'}, {'group': 'compliance', 'name': 'trestle-governance', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-governance', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '8eb4cfb58c5b68aa1e42b0ff9692901ce3cf31e778312f06f855b77197d77e6f'}, {'group': 'compliance', 'name': 'trestle-oscal-models', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-oscal-models', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '7bba543a60357ea15cb2d89d3033e8476955303d9a85ef75a7d6034a37c8d1e0'}, {'group': 'compliance', 'name': 'trestle-poam', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-poam', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': 'e55f92a69dd28687d174a4994c90bdf132611782022dbd522f4d53611e2d50db'}, {'group': 'compliance', 'name': 'trestle-task-system', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-task-system', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '95eb99987e8b80ac4e50af0e9620bfa7abe8a209af64dc927d0420ab40fa56b2'}, {'group': 'compliance', 'name': 'trestle-validation', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-validation', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': '111c29434296b4aac502c29786f8a4531a970c6f3be2d104ef2db38647ca9714'}, {'group': 'compliance', 'name': 'trestle-workspace', 'repo': 'oscal-compass-lab/compliance-trestle-skills', 'path': 'skills/trestle-workspace', 'commit': 'dee66a4feef00eb9bfddcf9ec6c557ff19903f86', 'sha256': 'f16a5481052eb84691968d2cfb9062491e84ab0e4dc856d9c5237d14b5ed6a65'}, {'group': 'compliance', 'name': 'compliance-check', 'repo': 'anthropics/knowledge-work-plugins', 'path': 'legal/skills/compliance-check', 'commit': '02ffa99ff00ce4d3046ccebabb8357cb39f3ab06', 'sha256': '8dff8ebe7eda9ad5e38f3faf1016a7fc1eeba65bcdb7f5ae45ea507824bbc1c2'}, {'group': 'compliance', 'name': 'policy-diff', 'repo': 'anthropics/claude-for-legal', 'path': 'regulatory-legal/skills/policy-diff', 'commit': '4a6c651889c97cc9140580363c73e0eb17379c2b', 'sha256': '01c2c516b4a99d1f9f64a1df3579bd50ab489b192416f2d4cd6957ebc4c200a5'}, {'group': 'compliance', 'name': 'gap-surfacer', 'repo': 'anthropics/claude-for-legal', 'path': 'regulatory-legal/skills/gap-surfacer', 'commit': '4a6c651889c97cc9140580363c73e0eb17379c2b', 'sha256': '56fc5441d1401d5ff21f9ccdf0e200341b823d26f8304dfd310dd0b89f706d3e'}, {'group': 'compliance', 'name': 'reg-feed-watcher', 'repo': 'anthropics/claude-for-legal', 'path': 'regulatory-legal/skills/reg-feed-watcher', 'commit': '4a6c651889c97cc9140580363c73e0eb17379c2b', 'sha256': 'db349c5fd51c0fd73347ca7369de4de805f16ebc9bf12f830c8dd17cbeb67744'}, {'group': 'learning', 'name': 'continuous-learning-v2', 'repo': 'affaan-m/ECC', 'path': 'skills/continuous-learning-v2', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': 'ddad12451a0d582c6ef1b197b7be193bef613cbc74eba4117fcb74fc7a4ecd00'}, {'group': 'learning', 'name': 'eval-harness', 'repo': 'affaan-m/ECC', 'path': 'skills/eval-harness', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': '3a5339a5a80099a5c9d55da9b6c8e05a690f5befc6f78d779c75dd79d818ca07'}, {'group': 'learning', 'name': 'agent-eval', 'repo': 'affaan-m/ECC', 'path': 'skills/agent-eval', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': '57fa683e356b6f5612bc6d7dc101ae39c126e6edf9a48f4ec53afea3e80d7e5d'}, {'group': 'learning', 'name': 'agent-self-evaluation', 'repo': 'affaan-m/ECC', 'path': 'skills/agent-self-evaluation', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': '96bb21eb37760f596789f84ec0a5dad4ef92fc2255e35cd7b88dc4234062b8c1'}, {'group': 'learning', 'name': 'iterative-retrieval', 'repo': 'affaan-m/ECC', 'path': 'skills/iterative-retrieval', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': 'b453b16d3e36174b8487c05ab807d0321a66c2d058812b2ae129d82824183725'}, {'group': 'memory', 'name': 'unified-memory', 'repo': 'affaan-m/ECC', 'path': 'skills/unified-memory', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': 'c42e96155783a44a50f13ab63e9d7f33c9d03b7a1f30777ef434401469c1630a'}, {'group': 'storage', 'name': 'rust-patterns', 'repo': 'affaan-m/ECC', 'path': 'skills/rust-patterns', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': '51379df2bd92a482309e109e5a210210842594f660cceeb2d68d18e995108edf'}, {'group': 'storage', 'name': 'rust-testing', 'repo': 'affaan-m/ECC', 'path': 'skills/rust-testing', 'commit': 'b92f181b3b748d1abbc2afd94af6aa7d8c8fca29', 'sha256': '02a9a40dcc42dd21d14730d29b13ee0a8b274694360585dc0fadb2602eb56ddf'}, {'group': 'protocols', 'name': 'grpc', 'repo': 'TerminalSkills/skills', 'path': 'skills/grpc', 'commit': 'a021875c0f1bd906248e3784ce470faa65055201', 'sha256': '7d5d3fa510ce9eb5dec5827cc8282e670c72a7e7fc30f1b242e691b5b0b9d7f3'}, {'group': 'network', 'name': 'wireguard', 'repo': 'TerminalSkills/skills', 'path': 'skills/wireguard', 'commit': 'a021875c0f1bd906248e3784ce470faa65055201', 'sha256': '90ac7564176c66665ba7f279f1f29cb7f7c6940b7ce9f561eea10c66e3082520'}, {'group': 'network', 'name': 'xray', 'repo': 'TerminalSkills/skills', 'path': 'skills/xray', 'commit': 'a021875c0f1bd906248e3784ce470faa65055201', 'sha256': 'b90ef053e0729154175eff3aeed91ef35c4f8d7cddc1b9de2cfd39337ba089f6'}, {'group': 'protocols', 'name': 'a2a-protocol', 'repo': 'TerminalSkills/skills', 'path': 'skills/a2a-protocol', 'commit': 'a021875c0f1bd906248e3784ce470faa65055201', 'sha256': '503be944e6f4deadbf8d4c5194ef61afb507fb122068b9ae1c36564665971032'}, {'group': 'generative-ui', 'name': 'sveltekit', 'repo': 'TerminalSkills/skills', 'path': 'skills/sveltekit', 'commit': 'a021875c0f1bd906248e3784ce470faa65055201', 'sha256': '26d7b5789a8917c0b416052f45dc16cd164e222b67b85508bfaab1716f093aa9'}, {'group': 'network', 'name': 'linux-network-admin', 'repo': 'peterbamuhigire/linux-skills', 'path': '03-networking-and-dns/linux-network-admin', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': '490ce9beebc8cab0a57f48eb8b1312b2fd58dc12749e7340ad011a723f7ffa70'}, {'group': 'storage', 'name': 'linux-disk-storage', 'repo': 'peterbamuhigire/linux-skills', 'path': '06-storage-and-filesystems/linux-disk-storage', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': 'cb2a628017595f4c550c67a382a058dc57931b3ebffa6ddd57d2396002e8e3e4'}, {'group': 'storage', 'name': 'linux-system-monitoring', 'repo': 'peterbamuhigire/linux-skills', 'path': '08-observability-and-logging/linux-system-monitoring', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': '3e54f17a99dd6bc5ca4714abe70183a388d4051863d00c96fbc62ade3156b0f5'}, {'group': 'storage', 'name': 'linux-troubleshooting', 'repo': 'peterbamuhigire/linux-skills', 'path': '09-troubleshooting-and-recovery/linux-troubleshooting', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': '0d998cdf6abe13e414c9a053560c71ed5e943f32b1b0e8620b5973b7e412ede1'}, {'group': 'storage', 'name': 'linux-filesystem-snapshots', 'repo': 'peterbamuhigire/linux-skills', 'path': '13-backup-and-archiving/linux-filesystem-snapshots', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': '6394303e2133c9d347ff75612b28718bb78121f9f4bda440daba88d488cf01c6'}, {'group': 'storage', 'name': 'linux-perf-profiling', 'repo': 'peterbamuhigire/linux-skills', 'path': '14-performance-and-kernel/linux-perf-profiling', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': '2e33c3144ed2d663bfcbd6c8c670e134071b8a7724c1ce1d6bc6ef823f6164a3'}, {'group': 'storage', 'name': 'linux-sysctl-tuning', 'repo': 'peterbamuhigire/linux-skills', 'path': '14-performance-and-kernel/linux-sysctl-tuning', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': '337e5f84906f87c8f92c306e78bd056d4d11310e303497a0cece00d0b5f34f5e'}, {'group': 'network', 'name': 'network-config-validation', 'repo': 'peterbamuhigire/linux-skills', 'path': '16-network-equipment/network-config-validation', 'commit': 'e874c08e73c31c873087f2901c11d9788ecaf54a', 'sha256': 'cc95b4ebaaa9f6ed6a62d05911b39dfcc1528466d642a04bce14afc96ebcf509'}, {'group': 'memory', 'name': 'kaeru', 'repo': 'LamantinAI/kaeru', 'path': 'skills/kaeru-skill', 'commit': 'ed1e98afc94d4d2ac1fe8c0cc109fee8cbc9619e', 'sha256': '68cc0f52fab17c6bfeccafa3ceedd32ca45c084ba26bec1713806cb20218a261'}, {'group': 'training', 'name': 'huggingface-llm-trainer', 'repo': 'huggingface/skills', 'path': 'skills/huggingface-llm-trainer', 'commit': 'c3ff942be3e07116b475253f0a8fc4f54682c1a8', 'sha256': 'fb5c5e25103e38226e4d8ae28db84a01de42a11da4be1918fa72f833699f9d0d'}, {'group': 'training', 'name': 'huggingface-datasets', 'repo': 'huggingface/skills', 'path': 'skills/huggingface-datasets', 'commit': 'c3ff942be3e07116b475253f0a8fc4f54682c1a8', 'sha256': 'eeca50adf211ea64a288b852938f85a5183b045c720dc696b62db9aa2ea8deb2'}, {'group': 'training', 'name': 'huggingface-trackio', 'repo': 'huggingface/skills', 'path': 'skills/huggingface-trackio', 'commit': 'c3ff942be3e07116b475253f0a8fc4f54682c1a8', 'sha256': '540f52974f4f77c2455a7b37ed983da0dbfb0f5af3d2c482e4736f14f1cc8724'}, {'group': 'training', 'name': 'train-sentence-transformers', 'repo': 'huggingface/skills', 'path': 'skills/train-sentence-transformers', 'commit': 'c3ff942be3e07116b475253f0a8fc4f54682c1a8', 'sha256': '9bd098d81049fc245032ee4f97cf5d9de906d726731ace4a6df0bad6d9dcd182'}, {'group': 'training', 'name': 'trl-training', 'repo': 'huggingface/skills', 'path': 'skills/trl-training', 'commit': 'c3ff942be3e07116b475253f0a8fc4f54682c1a8', 'sha256': '42c855bdc6171390abc7413cdb19bd4fd8dddfb31094d519a874427de08ddc2a'}, {'group': 'zerolang', 'name': 'zero', 'repo': 'vercel-labs/zerolang', 'path': 'skills/zero', 'commit': '7e1a64d27cc37671df31c6370890bce86f5135e1', 'sha256': 'c66ceac43509c87728a0cde6afa4a4ef620e003df9091af975f073928a1304ba'}]
GROUP_ORDER = ['generative-ui', 'coordination', 'protocols', 'network', 'compliance',
               'learning', 'memory', 'storage', 'verification', 'training', 'zerolang']
GAPS = '''Coverage notes:
  OVS/OpenFlow: no dedicated verified skill included; network skills are supporting coverage.
  CozoDB: kaeru covers a Cozo-backed memory application, not general Cozo/Datalog engineering.
  Btrfs: snapshots and Linux/Rust diagnostics are included; the 3tched transport/cache lifecycle
    still needs repository-specific instructions (ephemeral snapshots and bounded cleanup).
  Tag routing: Xray covers traffic tags; 3tched identity/policy tag semantics remain project-specific.
  Learning: instruction/memory learning and model-weight training are separate groups.
  Zero: the official Zerolang bootstrap skill is included. The installed Zero compiler serves
    version-matched language, graph, diagnostics, build, package, test, and stdlib topics.
  Svelte only: React/Next.js renderer skills and React-focused MCP/codegen/XState skills are excluded.
    Shared protocol/framework-neutral skills may mention React examples; use Svelte bindings only.
  Installing a skill does not start its services, training jobs, hooks, or network configuration.
'''
RUNTIME_NOTES = '''Additional runtime requirements are task-dependent and are NOT installed:
  zero: the Zero compiler/runtime; preserve the existing compiler version. Then run
    zero skills, zero skills get agent, and zero skills get language as needed.
  beads-workflow / beads-viewer: bd / bv tools.
  continuous-learning-v2: host-specific hooks; copying instructions does not activate them.
  unified-memory: ECC Memory Vault runtime and its configured integration.
  kaeru: its MCP server/application; this installer does not replace your Cozo architecture.
  trestle skills: Compliance Trestle / OSCAL tooling as needed by the task.
  regulatory skills: your policy corpus and any connected research/feed sources.
  Hugging Face training: Python/ML dependencies and credentials/compute where required.
  Linux, WireGuard, Xray and gRPC tools: install only when implementing the respective task.
  json-render / AI SDK / Svelte: application dependencies belong in the actual project.
'''

def say(message):
    print(message, flush=True)

def parser():
    p = argparse.ArgumentParser(
        description='Install 78 verified skills for 3tched, with Svelte generative UI first.',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='Groups: ' + ', '.join(GROUP_ORDER) + '\n\n' + GAPS + '\n' + RUNTIME_NOTES)
    scope = p.add_mutually_exclusive_group()
    scope.add_argument('--project', metavar='PATH', help='Project directory (default: current directory).')
    scope.add_argument('--global', dest='global_scope', action='store_true', help='Install for your user account.')
    p.add_argument('--agent', choices=['codex', 'opencode', 'claude-code', 'gemini-cli', 'all'],
                   default='codex', help='Target agent (default: codex; all means these four agents).')
    p.add_argument('--group', action='append', default=[], help='Select a group; repeat or comma-separate. Default: all groups.')
    p.add_argument('--all', action='store_true', help='Select all catalog groups (the default); does not select all agents.')
    p.add_argument('--list', action='store_true', help='List the selected skills and their source links; no writes/network.')
    p.add_argument('--dry-run', action='store_true', help='Preview installation targets; no writes/network.')
    p.add_argument('--replace-existing', action='store_true', help='Back up conflicting skill entries before replacement.')
    p.add_argument('--source-dir', metavar='PATH', help='Persistent source location (default: ~/.local/share/3tched-skills).')
    p.add_argument('--retries', type=int, default=2, help='Fetch attempts per repository (default: 2).')
    p.add_argument('--timeout', type=int, default=300, help='Maximum seconds per Git command (default: 300).')
    return p

def selected_rows(args, p):
    groups = set()
    for value in args.group:
        groups.update(x.strip() for x in value.split(',') if x.strip())
    if args.all and groups:
        p.error('--all cannot be combined with --group')
    unknown = groups - set(GROUP_ORDER)
    if unknown:
        p.error('Unknown group(s): ' + ', '.join(sorted(unknown)))
    if args.retries < 1 or args.timeout < 1:
        p.error('--retries and --timeout must be positive')
    chosen = [r for r in MANIFEST if not groups or r['group'] in groups]
    return sorted(chosen, key=lambda r: GROUP_ORDER.index(r['group']))

def destinations(args):
    project = Path(args.project or os.getcwd()).expanduser().resolve()
    home = Path.home()
    names = ['codex', 'opencode', 'claude-code', 'gemini-cli'] if args.agent == 'all' else [args.agent]
    locations = {
        'codex': home / '.agents/skills' if args.global_scope else project / '.agents/skills',
        'opencode': home / '.config/opencode/skills' if args.global_scope else project / '.agents/skills',
        'claude-code': home / '.claude/skills' if args.global_scope else project / '.claude/skills',
        'gemini-cli': home / '.gemini/skills' if args.global_scope else project / '.agents/skills',
    }
    # Several agents share the project .agents/skills discovery directory.
    return project, list(dict.fromkeys(locations[n] for n in names))

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def git_run(args, cwd, timeout):
    env = os.environ.copy()
    env.update({'GIT_TERMINAL_PROMPT': '0', 'GIT_LFS_SKIP_SMUDGE': '1'})
    cmd = ['git', '-c', 'core.hooksPath=/dev/null', '-c', 'protocol.file.allow=never'] + list(args)
    # No downloaded shell scripts, npm hooks or pip installations are executed.
    result = subprocess.run(cmd, cwd=str(cwd), env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(result.stdout.strip()[-2000:] or 'git command failed')
    return result.stdout.strip()

def validate_source(root, rows, timeout):
    expected = rows[0]['commit']
    if not root.is_dir() or root.is_symlink():
        raise RuntimeError('Source checkout is absent or is an unexpected symlink: ' + str(root))
    actual = git_run(['rev-parse', 'HEAD'], root, timeout)
    if actual != expected:
        raise RuntimeError('Cached checkout revision differs from the pinned commit: ' + str(root))
    if git_run(['status', '--porcelain', '--untracked-files=normal'], root, timeout):
        raise RuntimeError('Cached checkout has local edits; preserve them and choose a new --source-dir: ' + str(root))
    for row in rows:
        folder = (root / row['path']).resolve()
        try:
            folder.relative_to(root.resolve())
        except ValueError:
            raise RuntimeError('Skill path escapes its source checkout')
        path = folder / 'SKILL.md'
        if not path.is_file() or digest(path) != row['sha256']:
            raise RuntimeError('Missing or changed SKILL.md: ' + row['name'])
    return root

def prepare_repo(base, rows, args):
    repo, commit = rows[0]['repo'], rows[0]['commit']
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo) or not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise RuntimeError('Invalid embedded repository or revision')
    destination = base / 'sources' / repo.replace('/', '--') / commit
    if destination.exists() or destination.is_symlink():
        return validate_source(destination, rows, args.timeout)
    destination.parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(1, args.retries + 1):
        staging = Path(tempfile.mkdtemp(prefix='.fetch-', dir=str(destination.parent)))
        try:
            say(f'Fetching {repo} at {commit[:12]} (attempt {attempt}/{args.retries})')
            git_run(['init', '--quiet'], staging, args.timeout)
            git_run(['remote', 'add', 'origin', 'https://github.com/' + repo + '.git'], staging, args.timeout)
            git_run(['fetch', '--quiet', '--no-tags', '--depth=1', 'origin', commit], staging, args.timeout)
            git_run(['checkout', '--quiet', '--detach', 'FETCH_HEAD'], staging, args.timeout)
            validate_source(staging, rows, args.timeout)
            os.replace(staging, destination)
            return destination
        except (RuntimeError, subprocess.TimeoutExpired, OSError) as exc:
            if attempt == args.retries:
                raise RuntimeError(f'{repo}: {exc}') from exc
            say(f'Retrying {repo}: {exc}')
            time.sleep(min(2 * attempt, 5))
        finally:
            if staging.exists():
                shutil.rmtree(staging)
    raise RuntimeError('Repository fetch did not complete')

def install_link(source, dest, backup_root, replace):
    if dest.is_symlink() and dest.resolve() == source.resolve():
        return 'present', None
    backup = None
    if os.path.lexists(dest):
        if not replace:
            return 'conflict', 'Preserved existing entry: ' + str(dest)
        backup_root.mkdir(parents=True, exist_ok=True)
        backup = backup_root / dest.name
        if os.path.lexists(backup):
            raise RuntimeError('Backup already exists: ' + str(backup))
        shutil.move(str(dest), str(backup))
    try:
        dest.symlink_to(source, target_is_directory=True)
    except BaseException:
        if backup is not None and not os.path.lexists(dest):
            shutil.move(str(backup), str(dest))
        raise
    return 'installed', str(backup) if backup else None

def write_report(path, report):
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    os.replace(temp, path)

def main(argv=None):
    if sys.version_info < (3, 9):
        raise RuntimeError('Python 3.9 or newer is required')
    p = parser()
    args = p.parse_args(argv)
    rows = selected_rows(args, p)
    project, targets = destinations(args)
    base = Path(args.source_dir).expanduser().resolve() if args.source_dir else Path.home() / '.local/share/3tched-skills'
    for target in targets:
        try:
            base.relative_to(target.resolve())
        except ValueError:
            pass
        else:
            p.error('--source-dir must be outside the agent skill discovery directories')
    if args.list:
        for row in rows:
            say(f"{row['group']:16} {row['name']:36} {row['repo']}")
            say(f"  https://github.com/{row['repo']}/blob/{row['commit']}/{row['path']}/SKILL.md")
        say(f'\n{len(rows)} selected skills.\n\n' + GAPS)
        return 0
    say(f'Selected {len(rows)} skills. Frontend: Svelte 5 + TypeScript.')
    for group in GROUP_ORDER:
        count = sum(r['group'] == group for r in rows)
        if count:
            say(f'  {group}: {count}')
    for target in targets:
        say('Target: ' + str(target))
    say('Persistent sources: ' + str(base))
    if args.dry_run:
        for row in rows:
            say(f"Would install {row['name']} from {row['repo']}@{row['commit'][:12]}")
        say('\nDry run: no network calls or filesystem changes.\n\n' + GAPS)
        return 0
    if not shutil.which('git'):
        raise RuntimeError('git is required. Install it with your distribution package manager and rerun.')
    if not args.global_scope and not project.is_dir():
        raise RuntimeError('Project directory does not exist: ' + str(project))
    base.mkdir(parents=True, exist_ok=True)
    run_id = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ') + '-' + str(os.getpid())
    report_dir = base / 'reports'
    report_dir.mkdir(exist_ok=True)
    report_path = report_dir / (run_id + '.json')
    report = {'run_id': run_id, 'project': None if args.global_scope else str(project),
              'agent': args.agent, 'targets': [str(t) for t in targets],
              'selected': len(rows), 'frontend': 'Svelte 5 + TypeScript',
              'coverage_notes': GAPS, 'runtime_notes': RUNTIME_NOTES, 'entries': []}
    # Hold a process-scoped lock to prevent competing writes in this source store.
    with (base / 'install.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError('Another installer is using this source directory; rerun when it finishes.')
        for target in targets:
            target.mkdir(parents=True, exist_ok=True)
        repos = collections.OrderedDict()
        for row in rows:
            repos.setdefault(row['repo'], []).append(row)
        try:
            for repo, batch in repos.items():
                try:
                    root = prepare_repo(base, batch, args)
                except (RuntimeError, OSError, subprocess.TimeoutExpired) as exc:
                    say(f'FAILED: {repo}: {exc}')
                    for row in batch:
                        for target in targets:
                            report['entries'].append({**row, 'status': 'failed', 'target': str(target / row['name']), 'detail': str(exc)})
                    write_report(report_path, report)
                    continue
                for row in batch:
                    source = root / row['path']
                    for i, target in enumerate(targets):
                        dest = target / row['name']
                        try:
                            status, detail = install_link(source, dest, base / 'backups' / run_id / str(i), args.replace_existing)
                        except OSError as exc:
                            status, detail = 'failed', str(exc)
                        report['entries'].append({**row, 'status': status, 'target': str(dest), 'detail': detail})
                        say(f"{status.upper():9} {row['name']}")
                write_report(report_path, report)
        finally:
            write_report(report_path, report)
    counts = collections.Counter(e['status'] for e in report['entries'])
    say('\nResult: ' + ', '.join(f'{k}={v}' for k, v in sorted(counts.items())))
    say('Report: ' + str(report_path))
    say('\n' + GAPS)
    say(RUNTIME_NOTES)
    say('Rerun the same command to retry failures. Existing matching links are skipped.')
    say('Start a fresh agent session to discover the new skills. Load only the skills needed by each specialist.')
    say('Keep the persistent source directory: installed skills are symlinks into its pinned Git checkouts.')
    return 1 if counts['failed'] or counts['conflict'] else 0

if __name__ == '__main__':
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print('\nInterrupted. Completed installs remain; rerun to continue.', file=sys.stderr)
        sys.exit(130)
    except Exception as exc:
        print('ERROR: ' + str(exc), file=sys.stderr)
        sys.exit(1)

PYTHON_3TCHED_INSTALLER
