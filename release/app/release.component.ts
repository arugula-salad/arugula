// The app's release ships through release.publish in chant.config.ts, so
// this component deploys nothing. A cut needs one component to compile, as
// chant's own release member has (INTENTIUS/chant#4040).
export const release = { name: "app-release", dependsOn: [], deploy: [] };
