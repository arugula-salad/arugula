// A deploy's approval moved to shared/ (DK5, #386), where the platform page
// may import it too: it holds no device key. This keeps the old path.
export * from "../../shared/deploy-approval.ts";
