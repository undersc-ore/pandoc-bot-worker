import {
  BasicDeliver,
  BasicProperties,
  connect,
} from "https://deno.land/x/amqp@v0.17.0/mod.ts";

// NOTE: Using mongo version since this seems abadoned:
//       https://deno.land/x/bson@v0.1.3
import { Bson } from "https://deno.land/x/mongo@v0.30.0/mod.ts";

const amqp_url = Deno.env.get("AMQP_URL");
if (!amqp_url) {
  console.error("No AMQP_URL provided");
  Deno.exit(1);
}

const conn = await connect(amqp_url);

console.log(conn);

const channel = await conn.openChannel();

console.log(channel);

const inputQueueName = "pandoc-bot-jobs";
const outputQueueName = "pandoc-outputs";

await channel.declareQueue({ queue: inputQueueName });
await channel.declareQueue({ queue: outputQueueName });

console.log("declared queues");

interface ConvertRequest {
  chat_id: number;
  file: Bson.Binary;
  file_id: string;
  from_filetype: string;
  to_filetype: string;
}

await channel.consume(
  { queue: inputQueueName },
  async (args: BasicDeliver, _props: BasicProperties, data: Uint8Array) => {
    // Deserialize message and save the file
    // TODO: Pipe data directly into pandoc subprocess
    const req = Bson.deserialize(data) as ConvertRequest;

    console.log("Got request", req);
    await Deno.writeFile(req.file_id, req.file.buffer);

    // Ack to message queue
    await channel.ack({ deliveryTag: args.deliveryTag });

    // Log the request
    console.log("Consumed request: ", req);
    console.log(
      `Convert ${req.file_id} from ${req.from_filetype} to ${req.to_filetype} for chat ${req.chat_id}`,
    );

    // Construct subprocess arguments and run
    const out_filename = `${req.file_id}.out`;
    const cmd: string[] = [
      "pandoc",
      req.file_id,
      "-o",
      out_filename,
      "--from",
      req.from_filetype,
      "--to",
      req.to_filetype,
    ];
    const proc = Deno.run({ cmd, stdout: "piped" });
    const { code } = await proc.status();

    const proc_success = code == 0;
    if (proc_success) {
      // Read file
      const out_file_bytes = await Deno.readFile(out_filename);

      console.log(`Pandoc subprocess succeeded for ${req.file_id}`);

      // Reply to queue
      await channel.publish(
        { routingKey: outputQueueName },
        { contentType: "application/bson" },
        Bson.serialize({
          chat_id: req.chat_id,
          file: out_file_bytes,
          to_filetype: req.to_filetype,
        }),
      );

      console.log(`Replied to MQ for ${req.file_id}`);
    } else {
      const error_msg = new TextDecoder().decode(await proc.output());

      console.log(`Pandoc subprocess failed for ${req.file_id}`);
      console.log(error_msg);

      // Bad conversion, return errors
      Bson.serialize({
        chat_id: req.chat_id,
        error_msg,
      });
    }
  },
);
