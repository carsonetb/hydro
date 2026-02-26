import logging

import colorlog 

LOG_LEVEL = logging.DEBUG

FORMAT = "[%(asctime)s] [%(name)s] [%(levelname)s] %(message)s"
FORMAT_INTERNAL = "[%(asctime)s] [%(name)s] [INTERNAL %(levelname)s] [at %(lineno)d] %(message)s"

FORMATTER = colorlog.ColoredFormatter(FORMAT)
FORMATTER_INTERNAL = colorlog.ColoredFormatter(FORMAT_INTERNAL)

FILE_FORMATTER = logging.Formatter(FORMAT)
FILE_FORMATTER_INTERNAL = logging.Formatter(FORMAT_INTERNAL)

def create_logger(name: str, internal: bool = True, to_file: bool = True, file: str = "logs/hydro.log"):
    logger = logging.getLogger(name)
    stream_handler = colorlog.StreamHandler()
    stream_handler.formatter = FORMATTER_INTERNAL if internal else FORMATTER
    if to_file:
        file_handler = logging.FileHandler(file)
        file_handler.formatter = FILE_FORMATTER_INTERNAL if internal else FILE_FORMATTER
        logger.handlers.append(file_handler)
    logger.handlers.append(stream_handler)
