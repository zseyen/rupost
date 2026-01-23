# Simple API Examples

这是一个简单的 API 测试示例文档。

## 基础操作

### 获取用户信息

获取指定用户的详细信息。

```http
GET https://httpbin.org/get?user_id=123
Accept: application/json
```

### 创建新用户

```rest
POST https://httpbin.org/post
Content-Type: application/json

{
  "name": "张三",
  "email": "zhangsan@example.com",
  "age": 25
}
```

### 更新用户信息

```http
PUT https://httpbin.org/put
Content-Type: application/json

{
  "user_id": 123,
  "name": "李四"
}
```

## 查询操作

### 搜索用户

```rest
GET https://httpbin.org/get?q=developer&page=1&limit=10
```

### 删除用户

```http
DELETE https://httpbin.org/delete?user_id=123
```
