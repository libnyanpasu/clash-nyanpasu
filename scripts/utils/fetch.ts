export const downloadFileToBuffer = async (url: string): Promise<Buffer> => {
  try {
    const response = await fetch(url);

    const buffer = Buffer.from(await response.arrayBuffer());

    return buffer;
  } catch (error) {
    throw error;
  }
};
