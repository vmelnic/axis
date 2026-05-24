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
| `s3` | S3-compatible object storage | Parsed, not yet implemented at runtime |

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

For local storage with public access, the runtime mounts a static file server at the appropriate path.

### MAX_SIZE

Maximum file size in bytes. Optional -- if not set, no size limit is enforced.

### TYPES

List of allowed MIME types. Optional -- if not set, all types are accepted.

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

## Runtime Behavior (Local Backend)

1. A UUID-based filename is generated.
2. The file is written to `{bucket}/{prefix}/{uuid_filename}`.
3. For public storage, the file URL is `/files/{bucket}/{prefix}/{filename}`.
4. For private storage, the relative path is returned.

## Compiler Checks

- Storage names referenced in UPLOAD must be declared.
- UPLOAD sources (file expressions) are validated.
