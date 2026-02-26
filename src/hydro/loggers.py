import logging
import os
from pathlib import Path

import colorlog 

LOG_LEVEL = logging.DEBUG

LOG_COLORS = {
    "DEBUG": "cyan",
    "INFO": "green",
    "WARNING": "yellow",
    "ERROR": "red",
    "CRITICAL": "bg_red",
}

FORMATTER = colorlog.ColoredFormatter("[%(asctime)s] %(log_color)s[%(name)s] [%(bold)s%(levelname)s%(reset)s%(log_color)s] %(message)s", log_colors=LOG_COLORS)
FORMATTER_INTERNAL = colorlog.ColoredFormatter("[%(asctime)s] %(log_color)s[INTERNAL] [%(name)s] [%(bold)s%(levelname)s%(reset)s%(log_color)s] [at %(lineno)d] %(message)s", log_colors=LOG_COLORS)

FILE_FORMATTER = logging.Formatter("[%(asctime)s] [%(name)s] [%(levelname)s] %(message)s")
FILE_FORMATTER_INTERNAL = logging.Formatter("[%(asctime)s] [%(name)s] [INTERNAL %(levelname)s] [at %(lineno)d] %(message)s")

def create_logger(name: str, internal: bool = True, to_file: bool = True, file: str = "logs/hydro.log") -> logging.Logger:
    logger = colorlog.getLogger(name)
    stream_handler = colorlog.StreamHandler()
    stream_handler.formatter = FORMATTER_INTERNAL if internal else FORMATTER
    if to_file:
        if not Path(file).exists():
            os.makedirs(os.path.dirname(file))
            open(file, "w").close()
        file_handler = logging.FileHandler(file)
        file_handler.formatter = FILE_FORMATTER_INTERNAL if internal else FILE_FORMATTER
        logger.addHandler(file_handler)
    logger.addHandler(stream_handler)
    logger.level = LOG_LEVEL
    return logger

if __name__ == "__main__":
    logger = create_logger("Loggers", internal=False, to_file=False)
    logger.info("Detected loggers file ran independently, here is some example usage.")
    logger.debug("This is a debug message.")
    logger.info("This is an info message.")
    logger.warning("This is a warning message.")
    logger.error("This is an error message.")
    logger.critical("This is a critical message.")