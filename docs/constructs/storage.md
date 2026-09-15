# STORAGE

File storage backend for handling file uploads. Supports local filesystem and S3-compatible storage.

## Syntax

```axis
STORAGE <name>
  BACKEND <local | s3>
  BUCKET <bucket_name>
  [PREFIX <path_prefix>]
  [ACCESS <public | private>]
  [MAX_SIZE <bytes>]
  [TYPES <mime_type1> [mime_type2...]]
```

## Example

```axis
STORAGE avatars
  BACKEND local
  BUCKET uploads
  PREFIX avatars
  ACCESS public
  MAX_SIZE 5242880
  TYPES image/jpeg image/png image/webp

STORAGE documents
  BACKEND s3
  BUCKET my-app-docs
  PREFIX user-docs
  ACCESS private
  MAX_SIZE 10485760
  TYPES application/pdf
```

## Fields

### BACKEND

Storage backend type:

| Backend | Description | Status |
|---------|-------------|--------|
| `local` | Local filesystem | Fully implemented |
| `s3` | AWS S3 or an S3-compatible object store | Fully implemented |

### BUCKET

The bucket or directory name for storage.

For local backend, files are stored under the bucket path. For S3, this is the S3 bucket name.

### PREFIX

Optional path prefix within the bucket. Files are stored under `{bucket}/{prefix}/{filename}`.

### ACCESS

| Access | Description |
|--------|-------------|
| `public` | Files are served via HTTP at `/files/{bucket}/{prefix}/` |
| `private` | Files are not publicly accessible |

For local storage with public access, the runtime mounts the bucket directory at `/files/{storage_name}`. The returned URL includes the optional prefix and generated filename.

For public S3 storage, set `AXIS_STORAGE_<NAME>_PUBLIC_BASE_URL` (the storage name is uppercased and non-alphanumeric characters become `_`) or the shared `AXIS_S3_PUBLIC_BASE_URL`. Private S3 storage returns an `s3://bucket/key` locator.

### MAX_SIZE

Maximum file size in bytes. Optional -- if not set, no size limit is enforced.

### TYPES

List of allowed MIME types or file extensions. Optional -- if not set, all types are accepted. Axis inspects known file signatures and does not rely solely on the client-provided filename or content type.

## Usage in Flows

Files are uploaded using the `UPLOAD` step in a flow:

```axis
FLOW upload_avatar POST /users/:id/avatar
  REALM api
  AUTH session
  BODY MULTIPART AvatarUpload
    file BLOB REQUIRED

  UPLOAD body.file -> avatars AS upload_result

  UPDATE users
    WHERE id EQ path.id
    SET avatar_url upload_result
  AS user
  OR 404

  RETURN 200 user
```

### UPLOAD Step

```axis
UPLOAD <file_expr> -> <storage_name> AS <binding>
```

- `file_expr` -- expression resolving to file data (typically from a MULTIPART body)
- `storage_name` -- name of a declared STORAGE
- `binding` -- variable name bound to the resulting URL/path

### MULTIPART Body

File uploads require a MULTIPART body declaration:

```axis
BODY MULTIPART AvatarUpload
  file BLOB REQUIRED
  description MAYBE TEXT
```

The `BLOB` type represents binary file data.

## Runtime Behavior

1. A UUID-based filename is generated.
2. `MAX_SIZE` and `TYPES` are enforced before a write is attempted.
3. Local files are written to `{bucket}/{prefix}/{uuid_filename}`. Public URLs use `/files/{storage_name}/{prefix}/{filename}`; private uploads return the relative key.
4. S3 objects are written to `{prefix}/{uuid_filename}`. Public uploads return an HTTP URL and private uploads return `s3://bucket/key`.

S3 uses the standard `AWS_*` environment variables understood by the AWS SDK ecosystem. `AWS_ENDPOINT_URL_S3` and `AWS_ALLOW_HTTP=true` support services such as MinIO during local development. See [Runtime](../runtime.md#file-storage) for response URL and request-limit configuration.

## Compiler Checks

- Storage names referenced in UPLOAD must be declared.
- UPLOAD sources (file expressions) are validated.
