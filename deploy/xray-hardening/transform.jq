def require($ok; $why): if $ok then . else error($why) end;
def text: type == "string" and length > 0;
def uuid: type == "string" and test("^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$");
def b64($n): type == "string" and length == $n and test("^[A-Za-z0-9_-]+$");
def tags_ok: type == "array" and all(.[]; type == "object" and (.tag|text)) and ([.[].tag]|length == (unique|length));
def clients_ok: type == "array" and length > 0 and all(.[]; type == "object" and (.id|uuid)) and ([.[].id]|length == (unique|length));
def one($a; $tag): [$a[]|select(.tag == $tag)] | require(length == 1; "target tag missing or duplicated") | .[0];
def xmux: {maxConcurrency:1,maxConnections:0,cMaxReuseTimes:0,hMaxRequestTimes:"600-900",hMaxReusableSecs:"1800-3000",hKeepAlivePeriod:0};
def xhttp($p): {path:$p,mode:"packet-up",extra:{xPaddingBytes:"100-1000"}};
require(type == "array" and length == 1; "one source JSON object required") | .[0] |
require(type == "object"; "source must be an object") |
require((.inbounds|tags_ok) and (.outbounds|tags_ok); "source arrays require unique nonempty tags") |
require((($profile == "decoy-reality") and (.routing == null or ((.routing|type == "object") and (.routing.rules == null or (.routing.rules|type == "array"))))) or ((.routing|type == "object") and (.routing.rules|type == "array")); "expected explicit routing rules") |
require(($parameters|length == 1) and ($parameters[0]|type == "object"); "one private parameters object required") |
$parameters[0] as $p |
if $profile == "host-reality" then
  require(($p.client_id|uuid) and ($p.reality_password|b64(43)) and ($p.reality_short_id|type == "string" and test("^([0-9a-fA-F]{2}){1,8}$")) and ($p.mldsa65_verify|b64(2603)); "invalid REALITY client parameters") |
  require(($p.http_uds_path|type == "string" and (startswith("/run/") or . == "/opt/run-mounts/ghostbridge/xray-egress/http.sock") and endswith(".sock") and (contains("..")|not)); "HTTP UDS must be a supported private socket path") |
  require((any(.inbounds[]; .tag == "od106-http-egress" or .listen == $p.http_uds_path)|not) and (any(.outbounds[]; .tag == "od106-reality-egress")|not); "new tag or socket collides with existing source") |
  require((any(.routing.rules[]; ((.inboundTag // [])|index("od106-http-egress")) != null or .outboundTag == "od106-reality-egress")|not); "new routing tags already referenced") |
  .inbounds += [{tag:"od106-http-egress",listen:$p.http_uds_path,protocol:"http",settings:{}}] |
  .outbounds += [{tag:"od106-reality-egress",protocol:"vless",settings:{vnext:[{address:"127.0.0.1",port:19443,users:[{id:$p.client_id,encryption:"none",flow:"xtls-rprx-vision"}]}]},streamSettings:{network:"tcp",security:"reality",realitySettings:{show:false,fingerprint:"chrome",serverName:"assistant.ghostbridge.tech",password:$p.reality_password,shortId:$p.reality_short_id,mldsa65Verify:$p.mldsa65_verify}},mux:{enabled:false}}] |
  .routing.rules = [{type:"field",inboundTag:["od106-http-egress"],outboundTag:"od106-reality-egress"}] + .routing.rules
elif $profile == "decoy-reality" then
  one(.inbounds;"egress-reality") as $i |
  require($i.protocol == "vless" and $i.listen == "0.0.0.0" and $i.port == 9443 and ($i.settings.clients|clients_ok) and ($i.settings.clients|length == 1) and $i.settings.clients[0].flow == "xtls-rprx-vision" and $i.streamSettings.network == "tcp" and $i.streamSettings.security == "reality" and $i.streamSettings.realitySettings.dest == "127.0.0.1:443"; "unexpected decoy REALITY shape") |
  require(($i.streamSettings.realitySettings.privateKey|b64(43)) and ($i.streamSettings.realitySettings.shortIds|type == "array" and length == 1 and all(.[]; type == "string" and test("^([0-9a-fA-F]{2}){1,8}$"))); "expected existing private key and nonempty short ID") |
  require(($p.mldsa65_seed|b64(43)) and $p.mldsa65_seed != $i.streamSettings.realitySettings.privateKey; "invalid MLDSA seed") |
  (.inbounds[]|select(.tag == "egress-reality")|.streamSettings.realitySettings) |= (.serverNames=["assistant.ghostbridge.tech"] | .show=false | .minClientVer="26.3.27" | .maxTimeDiff=60000 | .mldsa65Seed=$p.mldsa65_seed)
elif $profile == "host-xhttp" then
  one(.inbounds;"xhttp-in") as $i |
  require($i.protocol == "vless" and $i.listen == "127.0.0.1" and $i.port == 8444 and ($i.settings.clients|clients_ok) and $i.settings.decryption == "none" and ($i.settings.fallbacks|type == "array") and $i.streamSettings.network == "xhttp" and $i.streamSettings.security == "none" and $i.streamSettings.xhttpSettings.path == "/xhttp" and $i.streamSettings.xhttpSettings.mode == "auto"; "unexpected host XHTTP shape") |
  require(($p.xhttp_path|type == "string" and test("^/[A-Za-z0-9_-]{24,128}$")) and ($p.xhttp_decryption|type == "string" and startswith("mlkem768x25519plus.") and (test("[[:space:]]")|not)); "invalid XHTTP private parameters") |
  (.inbounds[]|select(.tag == "xhttp-in")|.settings) |= (.decryption=$p.xhttp_decryption | del(.fallbacks)) |
  (.inbounds[]|select(.tag == "xhttp-in")|.streamSettings.xhttpSettings) = xhttp($p.xhttp_path)
elif $profile == "decoy-xhttp" then
  one(.outbounds;"tunnel") as $o |
  require($o.protocol == "vless" and ($o.settings.vnext|type == "array" and length == 1) and $o.settings.vnext[0].address == "10.0.0.2" and $o.settings.vnext[0].port == 8444 and ($o.settings.vnext[0].users|clients_ok) and ($o.settings.vnext[0].users|length == 1) and all($o.settings.vnext[0].users[]; .encryption == "none") and $o.streamSettings.network == "xhttp" and $o.streamSettings.security == "none"; "unexpected decoy XHTTP shape") |
  require(($p.xhttp_path|type == "string" and test("^/[A-Za-z0-9_-]{24,128}$")) and ($p.xhttp_encryption|type == "string" and startswith("mlkem768x25519plus.") and contains(".1rtt.") and (test("[[:space:]]")|not)); "invalid XHTTP encryption parameters") |
  (.outbounds[]|select(.tag == "tunnel")|.settings.vnext[0].users[].encryption) = $p.xhttp_encryption |
  (.outbounds[]|select(.tag == "tunnel")|.streamSettings.xhttpSettings) = (xhttp($p.xhttp_path) | .extra.xmux=xmux | .extra.downloadSettings={address:"192.168.255.1",port:8444,network:"xhttp",security:"none",xhttpSettings:(xhttp($p.xhttp_path)|.extra.xmux=xmux)}) |
  (.outbounds[]|select(.tag == "tunnel")|.mux) = {enabled:false}
else error("unknown profile") end
