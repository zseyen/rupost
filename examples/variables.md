# Variable System Test

### Test with Environment Variables

```http
@name get-request-with-vars
@assert status == 200
GET {{base_url}}/get?api_key={{api_key}}
Authorization: Bearer {{token}}
```

### POST with Variables

```http
@name post-request-with-vars
@assert status == 200
POST {{base_url}}/post
Content-Type: application/json

{
  "api_key": "{{api_key}}",
  "environment": "testing"
}
```
