import { chromium } from 'playwright';
import express from 'express';
import path from 'path';

const app = express();
const port = 3000;

const browser = await chromium.launch({
  headless: false,
  args: ["--no-sandbox", "--enable-gpu"]
});

const page = await browser.newPage();
const BASE_URL = 'http://127.0.0.1:3000';

app.get('/gpu_report', async (_req, res) => {
    let wait_for_report = page.waitForFunction(async () => {
      return window.gpu_report != null && await window.gpu_report()
    });

    await page.goto(BASE_URL);
    let report = (await wait_for_report).toString();

    res.status(200).send(report);
});

app.get('/run_test', async (req, res) => {
  let params = new URL(req.url, BASE_URL).searchParams;
  let name = params.get("name");

  let test_url = new URL(BASE_URL);
  test_url.search = new URLSearchParams({ name }).toString();
  await page.goto(test_url.toString());

  await Promise.race([
    page.waitForFunction(() => {
      return window.sessionStorage.test_success
    }).then(() => res.sendStatus(200)),
    page.waitForFunction(() => {
       return window.sessionStorage.test_failure
    }).then((message) => res.status(500).send(message.toString())),
  ]);
});

app.use('/', express.static(path.join(import.meta.dirname, '../dist')))

app.listen(port, () => {
  console.log(`WASM test server running at http://127.0.0.1:3000`)
});
