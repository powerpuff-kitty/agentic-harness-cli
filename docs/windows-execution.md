# Windows check-execution contract

Windows check execution remains disabled until the executor satisfies this contract. The merged Job Object primitive is an ownership building block, not evidence that execution is enabled.

## Ownership

Create one private Job Object per invocation, set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and assign the direct child before any command output is accepted. The supervisor owns the job handle and the child handle. Cleanup closes the job only after the direct child has been observed and waited; assignment or cleanup failure is an execution error.

## I/O and deadlines

Redirect stdout and stderr to inheritable pipes and drain both concurrently. Count bytes before retaining evidence and stop reading once the declared limit is exceeded. A timeout, cancellation, output limit, pipe error, or supervisor fault terminates the job, waits for the direct child, and records the corresponding status plus cleanup and recovery outcomes.

## Completion boundary

Execution is supported only after the child has a terminal status, the pipes have reached EOF or an explicit bounded failure, the direct child has been waited, and the Job Object cleanup has succeeded. Any missing observation remains an execution error; no successful result may be synthesized from the exit code alone.

## Required Windows tests

- immediate success and non-zero exit;
- descendant termination after timeout and cancellation;
- output-limit refusal while both streams are active;
- pipe close/read failure and job-assignment failure;
- direct-child wait failure and job-close failure;
- mutation/revalidation and invocation-budget failures around execution.

Until these cases pass on Windows CI, `execution_review::supported()` must continue to return false on Windows and reports must retain `unsupported` rather than a passing runtime result.
