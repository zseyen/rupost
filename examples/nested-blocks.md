# Nested Code Blocks Example

测试嵌套代码块的解析。

## 文档中包含代码示例

当你想在 Markdown 中展示如何写 HTTP 请求时，可以使用嵌套代码块：

下面是一个四反引号的例子（用于文档）：

````http
GET https://httpbin.org/get
````

这个不会被执行，因为它是展示用的。

## 实际执行的请求

### 正常的 HTTP 请求

```rest
GET https://httpbin.org/get
User-Agent: RuPost/Test
```

### 包含 JSON 响应示例的文档

有时候文档中需要展示 JSON 响应格式：

```http
GET https://httpbin.org/json
Accept: application/json
```

预期响应：
```json
{
  "slideshow": {
    "author": "Yours Truly",
    "title": "Sample Slide Show"
  }
}
```

### UUID 生成器

```rest
@name uuid-generator
@assert status == 200
@assert body.uuid exists
GET https://httpbin.org/uuid
```
