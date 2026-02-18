import init, { run_test, gpu_report } from "./test.js"


export function start() {
  init().then(async () => {
    window.gpu_report = gpu_report;

    let url = new URL(window.location.href);
    let name = url.searchParams.get("name");

    if (name != null) {
      await run_test(name)
    }
  });
}
