# 运营方签名凭证

实例可在 server.toml 中配置一个凭证签发方。商店和实名验证服务由运营方提供，服务端用该方的 Ed25519 公钥验签。签名私钥由签发方持有。未配置签发方时，准备或处理购买、提交身份凭证都会返回对应服务不可用；查询身份状态仍读取当前账号已记录的有效期。

```toml
[provider]
public_key_pem = "provider-public.pem"
issuer = "test-operator"
audience = "instance-a"

[provider.products.community_pack]
item = "community_token"
count = 5
```

`item` 引用本实例社区内容目录中的物品。不同实例使用各自的 audience。适配器需要能够把签发方返回的 JWT 传给客户端 RPC；原版客户端及商业支付渠道的购买流程仍需单独接入。

## 购买

1. 使用当前会话调用 `storePrepareIAP(session, {"product":"community_pack","requestId":"<uuid>"})`。同一账号重复提交同一 requestId 会返回原订单。结果含 orderId、product、expiresAt，订单有效一小时。
2. 签发方独立验证真实购买并核对账号和订单后签署 EdDSA JWT。标准字段为 iss、aud、sub（本实例玩家 UUID）、iat、exp。iat 和 exp 使用 Unix 秒，最长签名有效期为 300 秒。附加字段为 `purpose: "iap.purchase"`、`order`（返回的 orderId）、`product` 和 `receipt`。receipt 是签发方为该笔交易分配的稳定唯一、不含个人信息的标识符。
3. 调用 `storeProcessIAP(session, {"attestation":"<signed JWT>"})`。服务端核对账号、用途、产品和订单，原子记录交易摘要并向该玩家发放下单时配置的物品和数量。同一订单和交易可以重复提交，其他订单不能重用同一交易。已履约的订单仍可用重新签发的有效凭证重试。

## 身份状态

签发方验证身份后签发同样的标准 JWT 字段，附加 `purpose: "identity.verified"` 及 `valid_until`（身份认证状态有效期的 Unix 秒时间戳）。调用 `verifyGovernmentId(session, {"attestation":"<signed JWT>"})` 提交。之后 `isGovernmentIdVerified(session)` 返回 `{"verified":true}` 直到有效期结束。数据库保存玩家 UUID 和到期时间。支付记录保存带签发方命名空间的交易 SHA-256 摘要、订单和发放内容。接口不接收原始付款资料或证件照片。
